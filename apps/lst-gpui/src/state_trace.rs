//! Test-only state-trace channel. When `LST_X11_STATE_TRACE_FILE` is set,
//! the editor appends one JSON line per settled state to that file. The X11
//! harness reads this stream to assert on cursor positions, vim mode, find
//! state, status, and viewport geometry without typing-and-inspecting-the-
//! autosave-file.
//!
//! No-op (early return) when the env var is unset, so production binaries
//! pay nothing. Mirrors the existing `bench_trace` pattern in spirit, but
//! emits structured records instead of plain `key=value` lines because the
//! schema is rich enough to want serde.

use std::cell::Cell;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use gpui::{Bounds, Pixels, Window};
use lst_editor::find::FindScope;
use serde::Serialize;

use crate::{
    char_to_line_col, runtime::tab_identity, ClosePromptStatus, LstGpuiApp, QuitReviewDecision, QuitReviewItemStatus,
    WorkspaceSurface,
};

pub(crate) const STATE_TRACE_SCHEMA_VERSION: u32 = 8;

/// Holds the state-trace path and emitter state. Constructed once at app
/// init from the env var; subsequent calls to `try_emit` are no-ops when
/// no path was configured.
pub(crate) struct StateTraceEmitter {
    path: Option<PathBuf>,
    seq: Cell<u64>,
    /// Re-entrancy guard. `update_model` can recurse via clipboard reads or
    /// effect handlers. Without this guard a paste-into-multi-cursor would
    /// emit one record per cursor instead of one record per settled state.
    emitting: Cell<bool>,
}

impl StateTraceEmitter {
    pub(crate) fn from_env() -> Self {
        // Truncate at startup so the harness reads a fresh stream per
        // `Editor::spawn`; without this, the file would accumulate records
        // across runs and the harness would have to track its own start
        // offset. If the truncate fails (e.g. write-protected path), drop
        // the path so subsequent `try_emit` calls become silent no-ops
        // instead of spamming stderr on every state change.
        let path = env::var_os("LST_X11_STATE_TRACE_FILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .and_then(|path| match fs::write(&path, "") {
                Ok(()) => Some(path),
                Err(error) => {
                    eprintln!(
                        "lst_gpui state-trace: failed to truncate {}: {error} — disabling channel",
                        path.display()
                    );
                    None
                }
            });
        Self {
            path,
            seq: Cell::new(0),
            emitting: Cell::new(false),
        }
    }

    /// Build and append one record. The closure is called only when the
    /// emitter is active and we are not already emitting (re-entrancy
    /// guard). Invariant on the caller: build the record from settled
    /// model state — re-entrant calls are dropped silently.
    pub(crate) fn try_emit<F>(&self, build: F)
    where
        F: FnOnce(u64) -> StateTraceRecord,
    {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if self.emitting.get() {
            return;
        }
        self.emitting.set(true);
        let seq = self.seq.get();
        let record = build(seq);
        self.seq.set(seq + 1);
        if let Err(error) = append_record(path, &record) {
            eprintln!("lst_gpui state-trace: failed to append to {}: {error}", path.display());
        }
        self.emitting.set(false);
    }
}

fn append_record(path: &PathBuf, record: &StateTraceRecord) -> io::Result<()> {
    let mut line =
        serde_json::to_vec(record).map_err(|err| io::Error::other(format!("serialize state trace: {err}")))?;
    line.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)
}

#[derive(Serialize)]
pub(crate) struct StateTraceRecord {
    pub schema_version: u32,
    pub seq: u64,
    pub revision: u64,
    pub active_tab_index: usize,
    pub active_tab_id: u64,
    pub active_tab_path: Option<String>,
    pub active_tab_modified: bool,
    pub active_tab_backing_file_missing: bool,
    pub line_count: usize,
    pub cursors: Vec<TraceCursor>,
    pub primary_cursor_index: usize,
    pub marked_range: Option<TraceRange>,
    pub input_mode: &'static str,
    pub vim_mode: String,
    pub vim_pending: String,
    pub find: TraceFind,
    pub goto_line_input: Option<String>,
    pub recent_panel_open: bool,
    pub recent_panel_query: Option<String>,
    pub recent_panel_selected_path: Option<String>,
    pub recent_panel_empty_message: Option<String>,
    pub recent_panel_content_search_pending: bool,
    pub focused_input: &'static str,
    pub workspace_surface: &'static str,
    pub workspace_surface_selected_index: Option<usize>,
    pub settings_selected_item: Option<&'static str>,
    pub word_wrap_enabled: bool,
    pub close_prompt_file: Option<String>,
    pub close_prompt_status: Option<&'static str>,
    pub close_prompt_error: Option<String>,
    pub quit_review_open: bool,
    pub quit_review_items: Vec<TraceQuitReviewItem>,
    pub quit_review_selected_index: Option<usize>,
    pub quit_review_message: Option<String>,
    pub file_conflict_path: Option<String>,
    pub file_conflict_button_bounds_px: TraceFileConflictButtonBounds,
    pub cleanup_confirmation_open: bool,
    pub status_message: String,
    pub status_bar: String,
    pub app_menu_button_bounds_px: Option<(f32, f32, f32, f32)>,
    pub all_tabs_button_bounds_px: Option<(f32, f32, f32, f32)>,
    pub recent_button_bounds_px: Option<(f32, f32, f32, f32)>,
    pub new_tab_button_bounds_px: Option<(f32, f32, f32, f32)>,
    pub cleanup_button_bounds_px: Option<(f32, f32, f32, f32)>,
    pub theme_name: String,
    pub theme_button_bounds_px: Option<(f32, f32, f32, f32)>,
    pub viewport: TraceViewport,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TraceQuitReviewItem {
    pub identity: String,
    pub decision: &'static str,
    pub status: &'static str,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct TraceFileConflictButtonBounds {
    pub reload: Option<(f32, f32, f32, f32)>,
    pub keep_mine: Option<(f32, f32, f32, f32)>,
    pub save_as: Option<(f32, f32, f32, f32)>,
    pub dismiss: Option<(f32, f32, f32, f32)>,
}

#[derive(Serialize)]
pub(crate) struct TraceCursor {
    pub anchor_char: usize,
    pub head_char: usize,
    pub anchor_line: usize,
    pub anchor_col: usize,
    pub head_line: usize,
    pub head_col: usize,
}

#[derive(Serialize)]
pub(crate) struct TraceRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Serialize)]
pub(crate) struct TraceFind {
    pub visible: bool,
    pub show_replace: bool,
    pub query: String,
    pub error: Option<String>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub scope: &'static str,
    pub match_count: usize,
    pub active_index: Option<usize>,
    pub chip_bounds_px: TraceFindChipBounds,
}

#[derive(Serialize, Default)]
pub(crate) struct TraceFindChipBounds {
    pub case_sensitive: Option<(f32, f32, f32, f32)>,
    pub whole_word: Option<(f32, f32, f32, f32)>,
    pub regex: Option<(f32, f32, f32, f32)>,
    pub scope: Option<(f32, f32, f32, f32)>,
}

#[derive(Serialize, Default)]
pub(crate) struct TraceViewport {
    pub scale_factor: f32,
    pub bounds_origin_px: Option<(f32, f32)>,
    pub bounds_size_px: Option<(f32, f32)>,
    pub char_width_px: f32,
    pub line_height_px: f32,
    pub scroll_top_px: f32,
    pub scroll_left_px: f32,
    /// Window-local x of the first code char on an unwrapped line. See
    /// `viewport::ViewportGeometry::code_origin_x_at_paint` for details.
    pub code_origin_x_px: f32,
    pub gutter_width_px: f32,
    pub occurrence_highlights: Vec<TraceRange>,
    pub rows: Vec<TraceRow>,
}

#[derive(Serialize)]
pub(crate) struct TraceRow {
    pub logical_line: usize,
    pub top_px: f32,
    pub line_start_char: usize,
    pub display_end_char: usize,
    pub gutter_text: Option<String>,
}

impl LstGpuiApp {
    /// Append one record to the state-trace channel when one is configured.
    /// No-op in production. Re-entrant calls (during effect handling) are
    /// dropped by `StateTraceEmitter::try_emit`'s internal guard.
    pub(crate) fn emit_state_trace(&self, window: &Window) {
        self.state_trace
            .try_emit(|seq| self.build_state_trace_record(seq, window));
    }

    fn build_state_trace_record(&self, seq: u64, window: &Window) -> StateTraceRecord {
        let tab = self.active_tab();
        let buffer = tab.buffer();
        let selection_set = tab.selection_set();
        let cursors = selection_set
            .as_slice()
            .iter()
            .enumerate()
            .map(|(index, sel)| {
                let (anchor_line, anchor_col) = char_to_line_col(buffer, sel.anchor());
                let (head_line, head_col) = char_to_line_col(buffer, sel.head());
                let visible_col = (!sel.has_selection())
                    .then(|| tab.visible_column_for_selection(index))
                    .flatten();
                TraceCursor {
                    anchor_char: sel.anchor(),
                    head_char: sel.head(),
                    anchor_line,
                    anchor_col: visible_col.unwrap_or(anchor_col),
                    head_line,
                    head_col: visible_col.unwrap_or(head_col),
                }
            })
            .collect::<Vec<_>>();
        let marked_range = tab.marked_range().map(|r| TraceRange {
            start: r.start,
            end: r.end,
        });
        let find = self.model.find();
        let status_message = self
            .cleanup_message
            .clone()
            .unwrap_or_else(|| self.model.status().to_string());
        let status_bar = self.status_details();
        let recent_page = self.recent.page();
        let recent_panel_selected_path = recent_page
            .selected_index
            .and_then(|index| recent_page.visible.get(index))
            .map(|path| path.to_string_lossy().into_owned());
        let recent_panel_empty_message = recent_page.empty_message;
        let cleanup_button_bounds_px = self.cleanup_button_bounds_px.map(|bounds| {
            (
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            )
        });
        let app_menu_button_bounds_px = trace_bounds(self.app_menu_button_bounds_px);
        let all_tabs_button_bounds_px = trace_bounds(self.all_tabs_button_bounds_px);
        let recent_button_bounds_px = trace_bounds(self.recent_button_bounds_px);
        let new_tab_button_bounds_px = trace_bounds(self.new_tab_button_bounds_px);
        let theme_button_bounds_px = trace_bounds(self.theme_button_bounds_px);
        StateTraceRecord {
            schema_version: STATE_TRACE_SCHEMA_VERSION,
            seq,
            revision: tab.revision(),
            active_tab_index: self.model.active_index(),
            active_tab_id: tab.id().get(),
            active_tab_path: tab.path().map(|p| p.to_string_lossy().into_owned()),
            active_tab_modified: tab.modified(),
            active_tab_backing_file_missing: tab.backing_file_missing(),
            line_count: tab.line_count(),
            cursors,
            primary_cursor_index: selection_set.primary_index(),
            marked_range,
            input_mode: match self.model.input_mode() {
                lst_editor::InputMode::Standard => "standard",
                lst_editor::InputMode::Vim => "vim",
            },
            vim_mode: self.model.vim_mode().label().to_string(),
            vim_pending: self.model.vim_pending_display(),
            find: TraceFind {
                visible: find.visible,
                show_replace: find.show_replace,
                query: find.query.clone(),
                error: find.error.clone(),
                case_sensitive: find.case_sensitive,
                whole_word: find.whole_word,
                use_regex: find.use_regex,
                scope: match find.scope {
                    FindScope::Document => "document",
                    FindScope::Selection { .. } => "selection",
                },
                match_count: find.matches.len(),
                active_index: find.active,
                chip_bounds_px: TraceFindChipBounds {
                    case_sensitive: trace_bounds(self.find_chip_bounds_px.case_sensitive),
                    whole_word: trace_bounds(self.find_chip_bounds_px.whole_word),
                    regex: trace_bounds(self.find_chip_bounds_px.regex),
                    scope: trace_bounds(self.find_chip_bounds_px.scope),
                },
            },
            goto_line_input: self.model.goto_line().map(ToOwned::to_owned),
            recent_panel_open: self.recent.is_open(),
            recent_panel_query: self.recent.is_open().then(|| self.recent.query().to_string()),
            recent_panel_selected_path,
            recent_panel_empty_message,
            recent_panel_content_search_pending: self.recent.content_search_pending(),
            focused_input: self.state_trace_focus_label(window),
            workspace_surface: match self.workspace_surface {
                WorkspaceSurface::None => "none",
                WorkspaceSurface::CommandPalette => "command_palette",
                WorkspaceSurface::Settings => "settings",
                WorkspaceSurface::TabList => "tab_list",
                WorkspaceSurface::AppMenu => "app_menu",
                WorkspaceSurface::LanguageMenu => "language_menu",
                WorkspaceSurface::ContextMenu => "context_menu",
            },
            workspace_surface_selected_index: matches!(
                self.workspace_surface,
                WorkspaceSurface::TabList
                    | WorkspaceSurface::AppMenu
                    | WorkspaceSurface::LanguageMenu
                    | WorkspaceSurface::ContextMenu
            )
            .then(|| {
                let count = self.workspace_surface_item_count();
                (count > 0).then_some(self.workspace_surface_selected.min(count - 1))
            })
            .flatten(),
            settings_selected_item: (self.workspace_surface == WorkspaceSurface::Settings)
                .then(|| self.settings_selection.selected_id())
                .flatten(),
            word_wrap_enabled: self.model.show_wrap(),
            close_prompt_file: self
                .close_prompt
                .as_ref()
                .and_then(|prompt| self.model.tab_by_id(prompt.tab_id))
                .map(tab_identity),
            close_prompt_status: self.close_prompt.as_ref().map(|prompt| match &prompt.status {
                ClosePromptStatus::Reviewing => "reviewing",
                ClosePromptStatus::Saving => "saving",
                ClosePromptStatus::Failed(_) => "failed",
            }),
            close_prompt_error: self.close_prompt.as_ref().and_then(|prompt| match &prompt.status {
                ClosePromptStatus::Failed(message) => Some(message.clone()),
                _ => None,
            }),
            quit_review_open: self.quit_review.is_some(),
            quit_review_items: self
                .quit_review
                .as_ref()
                .map(|review| {
                    review
                        .items
                        .iter()
                        .map(|item| {
                            let (status, error) = match &item.status {
                                QuitReviewItemStatus::Pending => ("pending", None),
                                QuitReviewItemStatus::Saving => ("saving", None),
                                QuitReviewItemStatus::Saved => ("saved", None),
                                QuitReviewItemStatus::Failed(message) => ("failed", Some(message.clone())),
                            };
                            TraceQuitReviewItem {
                                identity: item.identity.clone(),
                                decision: match item.decision {
                                    QuitReviewDecision::Save => "save",
                                    QuitReviewDecision::Discard => "discard",
                                },
                                status,
                                error,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            quit_review_selected_index: self.quit_review.as_ref().map(|review| review.selected_index),
            quit_review_message: self.quit_review.as_ref().and_then(|review| review.message.clone()),
            file_conflict_path: self
                .active_file_conflict()
                .map(|notice| notice.path.to_string_lossy().into_owned()),
            file_conflict_button_bounds_px: TraceFileConflictButtonBounds {
                reload: trace_bounds(self.file_conflict_button_bounds_px.reload),
                keep_mine: trace_bounds(self.file_conflict_button_bounds_px.keep_mine),
                save_as: trace_bounds(self.file_conflict_button_bounds_px.save_as),
                dismiss: trace_bounds(self.file_conflict_button_bounds_px.dismiss),
            },
            cleanup_confirmation_open: self.cleanup_confirmation.is_some(),
            status_message,
            status_bar,
            app_menu_button_bounds_px,
            all_tabs_button_bounds_px,
            recent_button_bounds_px,
            new_tab_button_bounds_px,
            cleanup_button_bounds_px,
            theme_name: self.theme_name_rendered.clone(),
            theme_button_bounds_px,
            viewport: self.build_state_trace_viewport(window),
        }
    }

    fn state_trace_focus_label(&self, window: &Window) -> &'static str {
        if self.workspace_surface == WorkspaceSurface::CommandPalette
            && self.command_palette_focus_handle.is_focused(window)
        {
            "command_palette"
        } else if self.workspace_surface == WorkspaceSurface::Settings
            && (self.settings_search_focus_handle.is_focused(window) || self.surface_focus_handle.is_focused(window))
        {
            "settings"
        } else if self.workspace_surface == WorkspaceSurface::TabList && self.surface_focus_handle.is_focused(window) {
            "tab_list"
        } else if self.workspace_surface == WorkspaceSurface::AppMenu && self.surface_focus_handle.is_focused(window) {
            "app_menu"
        } else if self.workspace_surface == WorkspaceSurface::LanguageMenu
            && self.surface_focus_handle.is_focused(window)
        {
            "language_menu"
        } else if self.workspace_surface == WorkspaceSurface::ContextMenu
            && self.surface_focus_handle.is_focused(window)
        {
            "context_menu"
        } else if self.recent.is_open() && self.recent_focus_handle.is_focused(window) {
            "recent_query"
        } else if self.find_query_focus_handle.is_focused(window) {
            "find_query"
        } else if self.find_replace_focus_handle.is_focused(window) {
            "find_replace"
        } else if self.goto_line_focus_handle.is_focused(window) {
            "goto_line"
        } else if self.quit_review.is_some() && self.surface_focus_handle.is_focused(window) {
            "quit_review"
        } else if self.close_prompt.is_some() && self.surface_focus_handle.is_focused(window) {
            "close_prompt"
        } else if self.cleanup_confirmation.is_some() && self.surface_focus_handle.is_focused(window) {
            "cleanup_confirmation"
        } else if self.focus_handle.is_focused(window) {
            "editor"
        } else {
            "none"
        }
    }

    fn build_state_trace_viewport(&self, window: &Window) -> TraceViewport {
        let Some(view) = self.tab_views.get(&self.model.active_tab_id()) else {
            return TraceViewport::default();
        };
        let geometry = view.geometry.borrow();
        let (origin, size) = match geometry.bounds {
            Some(bounds) => (
                Some((f32::from(bounds.origin.x), f32::from(bounds.origin.y))),
                Some((f32::from(bounds.size.width), f32::from(bounds.size.height))),
            ),
            None => (None, None),
        };
        let buffer = self.active_tab().buffer();
        let len_chars = buffer.len_chars();
        let rows = geometry
            .rows
            .iter()
            .map(|row| TraceRow {
                logical_line: buffer.char_to_line(row.line_start_char.min(len_chars)),
                top_px: f32::from(row.row_top),
                line_start_char: row.line_start_char,
                display_end_char: row.display_end_char,
                gutter_text: row.gutter_text.clone(),
            })
            .collect::<Vec<_>>();
        TraceViewport {
            scale_factor: window.scale_factor(),
            bounds_origin_px: origin,
            bounds_size_px: size,
            char_width_px: f32::from(geometry.painted_char_width),
            line_height_px: f32::from(geometry.painted_row_height),
            scroll_top_px: f32::from(geometry.scroll_top_at_paint),
            scroll_left_px: f32::from(geometry.scroll_left_at_paint),
            code_origin_x_px: f32::from(geometry.code_origin_x_at_paint),
            gutter_width_px: f32::from(geometry.gutter_width_at_paint),
            occurrence_highlights: geometry
                .occurrence_highlights
                .iter()
                .map(|range| TraceRange {
                    start: range.start,
                    end: range.end,
                })
                .collect(),
            rows,
        }
    }
}

fn trace_bounds(bounds: Option<Bounds<Pixels>>) -> Option<(f32, f32, f32, f32)> {
    bounds.map(|bounds| {
        (
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    })
}
