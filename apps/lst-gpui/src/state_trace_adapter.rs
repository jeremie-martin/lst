use gpui::Window;
use lst_editor::find::FindScope;

use crate::{
    char_to_line_col, focus_trace_label,
    state_trace::{
        StateTraceRecord, TraceCursor, TraceFind, TraceRange, TraceRow, TraceViewport,
        STATE_TRACE_SCHEMA_VERSION,
    },
    LstGpuiApp,
};

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
        let status_bar = match self.selection_summary() {
            Some(sel) => format!("{} | {sel}", self.status_details()),
            None => self.status_details(),
        };
        let cleanup_button_bounds_px = self.cleanup_button_bounds_px.map(|bounds| {
            (
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            )
        });
        StateTraceRecord {
            schema_version: STATE_TRACE_SCHEMA_VERSION,
            seq,
            revision: tab.revision(),
            active_tab_index: self.model.active_index(),
            active_tab_id: tab.id().get(),
            active_tab_path: tab.path().map(|p| p.to_string_lossy().into_owned()),
            active_tab_modified: tab.modified(),
            line_count: tab.line_count(),
            cursors,
            primary_cursor_index: selection_set.primary_index(),
            marked_range,
            vim_mode: self.model.vim_mode().label().to_string(),
            vim_pending: self.model.vim_pending_display(),
            find: TraceFind {
                visible: find.visible,
                show_replace: find.show_replace,
                query: find.query.clone(),
                case_sensitive: find.case_sensitive,
                whole_word: find.whole_word,
                use_regex: find.use_regex,
                scope: match find.scope {
                    FindScope::Document => "document",
                    FindScope::Selection { .. } => "selection",
                },
                match_count: find.matches.len(),
                active_index: find.active,
            },
            goto_line_input: self.model.goto_line().map(ToOwned::to_owned),
            recent_panel_open: self.recent.is_open(),
            recent_panel_query: self
                .recent
                .is_open()
                .then(|| self.recent.query().to_string()),
            focused_input: self.state_trace_focus_label(),
            status_bar,
            cleanup_button_bounds_px,
            viewport: self.build_state_trace_viewport(window),
        }
    }

    fn state_trace_focus_label(&self) -> &'static str {
        if self.recent.is_open() {
            "recent_query"
        } else {
            focus_trace_label(self.focus_last_applied)
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
            rows,
        }
    }
}
