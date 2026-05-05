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

use serde::Serialize;

pub(crate) const STATE_TRACE_SCHEMA_VERSION: u32 = 1;

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
            eprintln!(
                "lst_gpui state-trace: failed to append to {}: {error}",
                path.display()
            );
        }
        self.emitting.set(false);
    }
}

fn append_record(path: &PathBuf, record: &StateTraceRecord) -> io::Result<()> {
    let mut line = serde_json::to_vec(record)
        .map_err(|err| io::Error::other(format!("serialize state trace: {err}")))?;
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
    pub line_count: usize,
    pub cursors: Vec<TraceCursor>,
    pub primary_cursor_index: usize,
    pub marked_range: Option<TraceRange>,
    pub vim_mode: String,
    pub vim_pending: String,
    pub find: TraceFind,
    pub goto_line_input: Option<String>,
    pub recent_panel_open: bool,
    pub recent_panel_query: Option<String>,
    pub focused_input: &'static str,
    pub status_bar: String,
    pub viewport: TraceViewport,
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
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub scope: &'static str,
    pub match_count: usize,
    pub active_index: Option<usize>,
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
    pub rows: Vec<TraceRow>,
}

#[derive(Serialize)]
pub(crate) struct TraceRow {
    pub logical_line: usize,
    pub top_px: f32,
    pub line_start_char: usize,
    pub display_end_char: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips_through_jsonl() {
        let record = StateTraceRecord {
            schema_version: STATE_TRACE_SCHEMA_VERSION,
            seq: 7,
            revision: 42,
            active_tab_index: 1,
            active_tab_id: 1234,
            active_tab_path: Some("/tmp/foo.txt".to_string()),
            active_tab_modified: true,
            line_count: 5,
            cursors: vec![TraceCursor {
                anchor_char: 0,
                head_char: 3,
                anchor_line: 0,
                anchor_col: 0,
                head_line: 0,
                head_col: 3,
            }],
            primary_cursor_index: 0,
            marked_range: None,
            vim_mode: "INSERT".to_string(),
            vim_pending: String::new(),
            find: TraceFind {
                visible: false,
                show_replace: false,
                query: String::new(),
                case_sensitive: false,
                whole_word: false,
                use_regex: false,
                scope: "document",
                match_count: 0,
                active_index: None,
            },
            goto_line_input: None,
            recent_panel_open: false,
            recent_panel_query: None,
            focused_input: "editor",
            status_bar: "INSERT | Ln 1 | Col 4".to_string(),
            viewport: TraceViewport::default(),
        };
        let line = serde_json::to_string(&record).expect("serialize");
        assert!(line.contains("\"vim_mode\":\"INSERT\""));
        assert!(line.contains("\"schema_version\":1"));
        assert!(line.contains("\"seq\":7"));
        assert!(line.contains("\"head_char\":3"));
        assert!(line.contains("\"focused_input\":\"editor\""));
    }
}
