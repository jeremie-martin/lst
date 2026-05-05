//! Reader for the editor's `LST_X11_STATE_TRACE_FILE` JSONL stream.
//!
//! The editor appends one [`StateTraceRecord`] per settled state. Tests
//! consume the stream through [`StateTraceReader`], which tracks a byte
//! offset across reads and tolerates a partially-written trailing line so
//! a paint that lands mid-poll never produces a garbage parse.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

pub const STATE_TRACE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StateTraceRecord {
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
    #[serde(default)]
    pub focused_input: String,
    pub status_bar: String,
    pub viewport: TraceViewport,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct TraceCursor {
    pub anchor_char: usize,
    pub head_char: usize,
    pub anchor_line: usize,
    pub anchor_col: usize,
    pub head_line: usize,
    pub head_col: usize,
}

impl TraceCursor {
    pub fn is_collapsed(&self) -> bool {
        self.anchor_char == self.head_char
    }

    pub fn anchor_pos(&self) -> (usize, usize) {
        (self.anchor_line, self.anchor_col)
    }

    pub fn head_pos(&self) -> (usize, usize) {
        (self.head_line, self.head_col)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct TraceRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TraceFind {
    pub visible: bool,
    pub show_replace: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub scope: String,
    pub match_count: usize,
    pub active_index: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TraceViewport {
    #[serde(default)]
    pub scale_factor: f32,
    pub bounds_origin_px: Option<(f32, f32)>,
    pub bounds_size_px: Option<(f32, f32)>,
    pub char_width_px: f32,
    pub line_height_px: f32,
    pub scroll_top_px: f32,
    pub scroll_left_px: f32,
    /// Window-local x where the first code char of an unwrapped line lives.
    /// Combine with `char_width_px` to convert col → x.
    #[serde(default)]
    pub code_origin_x_px: f32,
    pub rows: Vec<TraceRow>,
}

impl TraceViewport {
    /// Locate the painted display row that covers `logical_line`. When soft
    /// wrap is on, a logical line spans multiple rows; this returns the
    /// first matching row (the row containing column 0). Returns `None`
    /// when the line is not in the painted-rows window — callers must
    /// scroll-into-view first.
    pub fn first_row_for_line(&self, logical_line: usize) -> Option<&TraceRow> {
        self.rows
            .iter()
            .find(|row| row.logical_line == logical_line)
    }

    /// Convert a (line, col) text position into window-local pixels suitable
    /// for `Editor::click_at` and friends. Returns `None` when the line is
    /// not in the painted-rows window (caller must scroll-into-view first)
    /// or when geometry has not yet been captured (no paint has happened).
    ///
    /// Soft-wrap aware: walks every painted row of `logical_line` to find
    /// the one whose char range covers the requested column, then computes
    /// the pixel offset within that segment. Without this, clicking on a
    /// column past the wrap point would produce coordinates outside the
    /// painted glyphs.
    pub fn text_to_window_local(&self, line: usize, col: usize) -> Option<(i32, i32)> {
        if self.char_width_px <= 0.0 || self.line_height_px <= 0.0 {
            return None;
        }
        // Painted rows of `line` arrive in segment order (top-down) and the
        // first one's `line_start_char` is the buffer offset of column 0,
        // letting us anchor the target char without a buffer cross-reference.
        // Walk rows once: capture the first match to anchor `target_char`,
        // then prefer the segment that actually covers it.
        let mut first: Option<&TraceRow> = None;
        let mut target_char = 0usize;
        let mut covering: Option<&TraceRow> = None;
        for row in &self.rows {
            if row.logical_line != line {
                continue;
            }
            if first.is_none() {
                first = Some(row);
                target_char = row.line_start_char + col;
            }
            if target_char >= row.line_start_char && target_char <= row.display_end_char {
                covering = Some(row);
                break;
            }
        }
        let row = covering.or(first)?;
        let col_in_row = target_char.saturating_sub(row.line_start_char);
        let x = self.code_origin_x_px
            + (col_in_row as f32) * self.char_width_px
            + self.char_width_px * 0.5;
        let y = row.top_px + self.line_height_px * 0.5;
        let scale = if self.scale_factor > 0.0 {
            self.scale_factor
        } else {
            1.0
        };
        Some(((x * scale).round() as i32, (y * scale).round() as i32))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct TraceRow {
    pub logical_line: usize,
    pub top_px: f32,
    pub line_start_char: usize,
    pub display_end_char: usize,
}

/// Reads a JSONL trace file produced by the editor under
/// `LST_X11_STATE_TRACE_FILE`. The reader tracks a byte offset across calls
/// so [`Self::read_new_records`] only returns records appended since the
/// previous read.
pub struct StateTraceReader {
    path: PathBuf,
    offset: u64,
    buffered_partial: Vec<u8>,
    last_record: Option<StateTraceRecord>,
}

impl StateTraceReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            buffered_partial: Vec::new(),
            last_record: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn last_observed(&self) -> Option<&StateTraceRecord> {
        self.last_record.as_ref()
    }

    /// Drain all records appended since the previous call. Tolerates a
    /// partially-written trailing line — bytes after the last `\n` are
    /// stashed and re-tried on the next call.
    pub fn read_new_records(&mut self) -> Result<Vec<StateTraceRecord>> {
        let mut file = match OpenOptions::new().read(true).open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        let size = metadata.len();
        if size < self.offset {
            // File was truncated (e.g. a new editor run started). Restart
            // from the top.
            self.offset = 0;
            self.buffered_partial.clear();
        }
        if size == self.offset && self.buffered_partial.is_empty() {
            return Ok(Vec::new());
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut new_bytes = Vec::with_capacity((size - self.offset) as usize);
        file.read_to_end(&mut new_bytes)?;
        self.offset = size;

        let mut combined = std::mem::take(&mut self.buffered_partial);
        combined.extend_from_slice(&new_bytes);

        let mut records = Vec::new();
        let mut line_start = 0usize;
        for (idx, byte) in combined.iter().enumerate() {
            if *byte == b'\n' {
                let line = &combined[line_start..idx];
                if !line.is_empty() {
                    let record: StateTraceRecord = serde_json::from_slice(line).map_err(|err| {
                        format!(
                            "state-trace parse error at offset {} in {}: {err}; line: {}",
                            line_start,
                            self.path.display(),
                            String::from_utf8_lossy(line),
                        )
                    })?;
                    records.push(record);
                }
                line_start = idx + 1;
            }
        }
        if line_start < combined.len() {
            self.buffered_partial = combined[line_start..].to_vec();
        }
        if let Some(record) = records.last() {
            self.last_record = Some(record.clone());
        }
        Ok(records)
    }

    /// Drain the stream and return the most recent observed record. When no
    /// new record was appended since the previous read, returns the last
    /// record already observed by this reader.
    ///
    /// Returns an error only when no record has ever been observed — tests
    /// that call this should have driven at least one settled state through
    /// the editor first.
    pub fn latest(&mut self) -> Result<StateTraceRecord> {
        let records = self.read_new_records()?;
        records
            .into_iter()
            .next_back()
            .or_else(|| self.last_record.clone())
            .ok_or_else(|| {
                format!(
                    "no state-trace records available at {}; either the editor has not emitted yet or LST_X11_STATE_TRACE_FILE was not wired into spawn",
                    self.path.display()
                )
                .into()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::time::SystemTime;

    fn temp_trace_path(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("lst-x11-harness-state-trace-{label}-{stamp}"))
    }

    fn sample_record_line(seq: u64) -> String {
        format!(
            r#"{{"schema_version":1,"seq":{seq},"revision":{seq},"active_tab_index":0,"active_tab_id":1,"active_tab_path":null,"active_tab_modified":false,"line_count":1,"cursors":[{{"anchor_char":0,"head_char":0,"anchor_line":0,"anchor_col":0,"head_line":0,"head_col":0}}],"primary_cursor_index":0,"marked_range":null,"vim_mode":"INSERT","vim_pending":"","find":{{"visible":false,"show_replace":false,"query":"","case_sensitive":false,"whole_word":false,"use_regex":false,"scope":"document","match_count":0,"active_index":null}},"goto_line_input":null,"recent_panel_open":false,"recent_panel_query":null,"status_bar":"INSERT","viewport":{{"bounds_origin_px":null,"bounds_size_px":null,"char_width_px":0.0,"line_height_px":0.0,"scroll_top_px":0.0,"scroll_left_px":0.0,"rows":[]}}}}"#
        )
    }

    #[test]
    fn reader_returns_empty_for_missing_file() {
        let path = temp_trace_path("missing");
        let mut reader = StateTraceReader::new(&path);
        let records = reader.read_new_records().unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn reader_drains_records_and_advances_offset() {
        let path = temp_trace_path("drain");
        let _ = fs::remove_file(&path);
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "{}", sample_record_line(0)).unwrap();
        writeln!(file, "{}", sample_record_line(1)).unwrap();
        drop(file);

        let mut reader = StateTraceReader::new(&path);
        let first = reader.read_new_records().unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].seq, 0);
        assert_eq!(first[1].seq, 1);

        // Second call without further writes: empty.
        let second = reader.read_new_records().unwrap();
        assert!(second.is_empty());

        // Append a new record; the reader picks up only the new one.
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", sample_record_line(2)).unwrap();
        drop(file);

        let third = reader.read_new_records().unwrap();
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].seq, 2);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn latest_reuses_last_observed_record_when_no_new_lines_arrive() {
        let path = temp_trace_path("latest-cache");
        let _ = fs::remove_file(&path);
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "{}", sample_record_line(0)).unwrap();
        drop(file);

        let mut reader = StateTraceReader::new(&path);
        let first = reader.latest().unwrap();
        assert_eq!(first.seq, 0);

        let second = reader.latest().unwrap();
        assert_eq!(second.seq, 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reader_tolerates_partial_trailing_line() {
        let path = temp_trace_path("partial");
        let _ = fs::remove_file(&path);
        let line = sample_record_line(0);

        // Write half a line first. The sample line is pure ASCII, so a byte
        // split is safe; using `as_bytes` directly sidesteps clippy's
        // UTF-8-boundary warning.
        let mut file = fs::File::create(&path).unwrap();
        let half = line.len() / 2;
        file.write_all(&line.as_bytes()[..half]).unwrap();
        drop(file);

        let mut reader = StateTraceReader::new(&path);
        let first = reader.read_new_records().unwrap();
        assert!(first.is_empty(), "partial line should not yield a record");

        // Complete the line.
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&line.as_bytes()[half..]).unwrap();
        file.write_all(b"\n").unwrap();
        drop(file);

        let second = reader.read_new_records().unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].seq, 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reader_resets_when_file_is_truncated() {
        let path = temp_trace_path("truncate");
        let _ = fs::remove_file(&path);
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "{}", sample_record_line(99)).unwrap();
        drop(file);

        let mut reader = StateTraceReader::new(&path);
        let first = reader.read_new_records().unwrap();
        assert_eq!(first.len(), 1);

        // Truncate (simulating a new editor session).
        fs::write(&path, "").unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", sample_record_line(0)).unwrap();
        drop(file);

        let second = reader.read_new_records().unwrap();
        assert_eq!(second.len(), 1, "truncate should reset the offset");
        assert_eq!(second[0].seq, 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn latest_drains_to_newest_record() {
        let path = temp_trace_path("latest");
        let _ = fs::remove_file(&path);
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "{}", sample_record_line(10)).unwrap();
        writeln!(file, "{}", sample_record_line(11)).unwrap();
        writeln!(file, "{}", sample_record_line(12)).unwrap();
        drop(file);

        let mut reader = StateTraceReader::new(&path);
        let latest = reader.latest().unwrap();
        assert_eq!(latest.seq, 12);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn latest_errors_when_no_records_have_been_emitted() {
        let path = temp_trace_path("empty");
        let _ = fs::remove_file(&path);
        let _ = fs::File::create(&path).unwrap(); // empty file
        let mut reader = StateTraceReader::new(&path);
        assert!(reader.latest().is_err());
        let _ = fs::remove_file(&path);
    }
}
