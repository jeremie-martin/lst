use crate::{
    document::{char_to_position, position_to_char},
    history::{EditHistory, HistorySnapshot},
    language::{self, Language},
    selection::{CursorGoal, Position, Selection, SelectionSet, SelectionState},
    transaction::{
        apply_change_to_buffer, clamped_range, inserted_relative_range, EditOutcome, EditRequest, SelectionAfter,
        TextChange,
    },
};
use ropey::Rope;
use std::{
    fs::Metadata,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanguageMode {
    #[default]
    Auto,
    PlainText,
    Language(Language),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileStamp {
    len: u64,
    modified_unix_nanos: Option<i128>,
}
impl FileStamp {
    pub const fn from_raw(len: u64, modified_unix_nanos: Option<i128>) -> Self {
        Self {
            len,
            modified_unix_nanos,
        }
    }
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified_unix_nanos: metadata.modified().ok().map(system_time_to_unix_nanos),
        }
    }
}

/// What must be true about a save target before it may be replaced.
///
/// Ordinary saves preserve the last observed disk state, including an
/// observed deletion. Save As is the only unguarded write path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveExpectation {
    Matching(FileStamp),
    Absent,
    Unguarded,
}

fn system_time_to_unix_nanos(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos().min(i128::MAX as u128) as i128,
        Err(err) => -(err.duration().as_nanos().min(i128::MAX as u128) as i128),
    }
}
/// A single replacement applied to a buffer.
///
/// `range` is in character offsets relative to the buffer's state *before*
/// the batch this edit belongs to was applied. Ranges in a batch are sorted
/// ascending by `start` and non-overlapping, so consumers can apply them in
/// reverse order without re-mapping offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

/// How the buffer changed since the last observation.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum BufferDelta {
    /// Buffer text is identical to the prior observation.
    #[default]
    Unchanged,
    /// Buffer changed via a batch of edits whose offsets are valid against
    /// the prior buffer state. Consumers should apply them in reverse order.
    Edits(Vec<BufferEdit>),
    /// Buffer was replaced wholesale (undo/redo, file reload, save body
    /// reapply). Consumers should reparse from scratch.
    FullReplace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabId(u64);
impl TabId {
    pub fn from_raw(id: u64) -> Self {
        Self(id)
    }
    pub fn get(self) -> u64 {
        self.0
    }
}
#[derive(Clone)]
struct CachedLines {
    revision: u64,
    lines: Arc<[DisplayLine]>,
}

/// Immutable, cheaply cloned display text for one logical line.
pub type DisplayLine = Arc<str>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind {
    Regular,
    Scratchpad,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabOrigin {
    Untitled,
    Saved {
        path: PathBuf,
        file_stamp: Option<FileStamp>,
        kind: SaveKind,
        backing_file_missing: bool,
        suppressed_conflict_stamp: Option<FileStamp>,
    },
}
impl TabOrigin {
    fn saved(path: PathBuf, file_stamp: Option<FileStamp>, kind: SaveKind) -> Self {
        Self::Saved {
            path,
            file_stamp,
            kind,
            backing_file_missing: false,
            suppressed_conflict_stamp: None,
        }
    }
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Untitled => None,
            Self::Saved { path, .. } => Some(path),
        }
    }
    pub fn file_stamp(&self) -> Option<FileStamp> {
        match self {
            Self::Untitled => None,
            Self::Saved { file_stamp, .. } => *file_stamp,
        }
    }
    pub fn is_scratchpad(&self) -> bool {
        matches!(
            self,
            Self::Saved {
                kind: SaveKind::Scratchpad,
                ..
            }
        )
    }
    pub fn backing_file_missing(&self) -> bool {
        matches!(
            self,
            Self::Saved {
                backing_file_missing: true,
                ..
            }
        )
    }
    fn mark_backing_file_missing(&mut self) {
        if let Self::Saved {
            file_stamp,
            backing_file_missing,
            suppressed_conflict_stamp,
            ..
        } = self
        {
            *file_stamp = None;
            *backing_file_missing = true;
            *suppressed_conflict_stamp = None;
        }
    }
    pub fn conflict_suppressed_for(&self, stamp: FileStamp) -> bool {
        match self {
            Self::Untitled => false,
            Self::Saved {
                suppressed_conflict_stamp,
                ..
            } => *suppressed_conflict_stamp == Some(stamp),
        }
    }
    pub fn has_suppressed_conflict(&self) -> bool {
        matches!(
            self,
            Self::Saved {
                suppressed_conflict_stamp: Some(_),
                ..
            }
        )
    }
    fn mark_saved(&mut self, path: PathBuf, file_stamp: FileStamp) {
        let kind = if self.is_scratchpad() {
            SaveKind::Scratchpad
        } else {
            SaveKind::Regular
        };
        *self = Self::saved(path, Some(file_stamp), kind);
    }
    fn mark_saved_as(&mut self, path: PathBuf, file_stamp: FileStamp) {
        *self = Self::saved(path, Some(file_stamp), SaveKind::Regular);
    }
    fn update_file_stamp(&mut self, file_stamp: FileStamp) {
        if let Self::Saved {
            file_stamp: stamp,
            backing_file_missing,
            suppressed_conflict_stamp,
            ..
        } = self
        {
            *stamp = Some(file_stamp);
            *backing_file_missing = false;
            *suppressed_conflict_stamp = None;
        }
    }
    fn suppress_file_conflict(&mut self, stamp: FileStamp) {
        if let Self::Saved {
            suppressed_conflict_stamp,
            ..
        } = self
        {
            *suppressed_conflict_stamp = Some(stamp);
        }
    }
}
#[derive(Clone)]
pub struct EditorTab {
    id: TabId,
    name_hint: String,
    origin: TabOrigin,
    language: Option<Language>,
    language_mode: LanguageMode,
    buffer: Rope,
    content_epoch: u64,
    saved_content_epoch: u64,
    next_content_epoch: u64,
    selection: SelectionState,
    revision: u64,
    line_cache: Option<CachedLines>,
    history: EditHistory,
    last_edit_position: Option<usize>,
    marked_range: Option<Range<usize>>,
    /// Bookmarked logical lines, sorted ascending. Stored as raw line
    /// numbers — no anchor tracking, so bookmarks drift on edits.
    bookmarks: Vec<usize>,
    buffer_delta: BufferDelta,
}
impl EditorTab {
    pub fn empty(id: TabId, name_hint: String) -> Self {
        Self::from_text_with_stamp(id, name_hint, None, "", None)
    }
    pub fn from_path_with_stamp(id: TabId, path: PathBuf, text: &str, file_stamp: Option<FileStamp>) -> Self {
        let name_hint = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled")
            .to_string();
        Self::from_text_with_stamp(id, name_hint, Some(path), text, file_stamp)
    }
    pub fn scratchpad_with_stamp(id: TabId, path: PathBuf, file_stamp: FileStamp) -> Self {
        let name_hint = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("scratchpad")
            .to_string();
        Self::from_origin(
            id,
            name_hint,
            TabOrigin::saved(path, Some(file_stamp), SaveKind::Scratchpad),
            "",
        )
    }
    fn from_text_with_stamp(
        id: TabId,
        name_hint: String,
        path: Option<PathBuf>,
        text: &str,
        file_stamp: Option<FileStamp>,
    ) -> Self {
        let origin = path.map_or(TabOrigin::Untitled, |p| {
            TabOrigin::saved(p, file_stamp, SaveKind::Regular)
        });
        Self::from_origin(id, name_hint, origin, text)
    }
    fn from_origin(id: TabId, name_hint: String, origin: TabOrigin, text: &str) -> Self {
        let language = language::detect(origin.path().map(PathBuf::as_path), text.split('\n').next());
        Self {
            id,
            name_hint,
            origin,
            language,
            language_mode: LanguageMode::Auto,
            buffer: Rope::from_str(text),
            content_epoch: 0,
            saved_content_epoch: 0,
            next_content_epoch: 1,
            selection: SelectionState::single(Selection::collapsed(0)),
            revision: 0,
            line_cache: None,
            history: EditHistory::new(),
            last_edit_position: None,
            marked_range: None,
            bookmarks: Vec::new(),
            buffer_delta: BufferDelta::Unchanged,
        }
    }
    pub fn id(&self) -> TabId {
        self.id
    }
    pub(crate) fn set_id(&mut self, id: TabId) {
        self.id = id;
    }
    pub fn path(&self) -> Option<&PathBuf> {
        self.origin.path()
    }
    pub fn language(&self) -> Option<Language> {
        self.language
    }
    pub fn language_mode(&self) -> LanguageMode {
        self.language_mode
    }
    pub(crate) fn set_language_mode(&mut self, mode: LanguageMode) {
        self.language_mode = mode;
        self.refresh_language();
    }
    pub fn language_config(&self) -> &'static crate::language::LanguageConfig {
        language::config_for(self.language)
    }
    pub fn file_stamp(&self) -> Option<FileStamp> {
        self.origin.file_stamp()
    }
    pub fn save_expectation(&self) -> SaveExpectation {
        match self.file_stamp() {
            Some(stamp) => SaveExpectation::Matching(stamp),
            None => SaveExpectation::Absent,
        }
    }
    pub fn is_scratchpad(&self) -> bool {
        self.origin.is_scratchpad()
    }
    pub fn backing_file_missing(&self) -> bool {
        self.origin.backing_file_missing()
    }
    pub fn scratchpad_path(&self) -> Option<&PathBuf> {
        self.is_scratchpad().then(|| self.path()).flatten()
    }
    pub fn conflict_suppressed_for(&self, stamp: FileStamp) -> bool {
        self.origin.conflict_suppressed_for(stamp)
    }
    pub fn has_suppressed_conflict(&self) -> bool {
        self.origin.has_suppressed_conflict()
    }
    pub fn buffer(&self) -> &Rope {
        &self.buffer
    }
    pub fn selection(&self) -> Selection {
        self.selection.primary()
    }
    pub fn selection_set(&self) -> &SelectionSet {
        self.selection.selection_set()
    }
    pub(crate) fn selection_state(&self) -> &SelectionState {
        &self.selection
    }
    pub fn selection_reversed(&self) -> bool {
        self.selection().is_reversed()
    }
    pub fn marked_range(&self) -> Option<&Range<usize>> {
        self.marked_range.as_ref()
    }
    /// Returns `true` when the line was just bookmarked, `false` when an
    /// existing bookmark on that line was cleared.
    pub(crate) fn toggle_bookmark_at_cursor(&mut self) -> bool {
        let line = self.buffer.char_to_line(self.cursor_char());
        match self.bookmarks.binary_search(&line) {
            Ok(index) => {
                self.bookmarks.remove(index);
                false
            }
            Err(index) => {
                self.bookmarks.insert(index, line);
                true
            }
        }
    }
    /// Bookmark line strictly after `from_line`, wrapping to the first
    /// bookmark when none lies after the cursor. Returns `None` when no
    /// bookmarks exist.
    pub fn next_bookmark_line(&self, from_line: usize) -> Option<usize> {
        if self.bookmarks.is_empty() {
            return None;
        }
        self.bookmarks
            .iter()
            .copied()
            .find(|line| *line > from_line)
            .or_else(|| self.bookmarks.first().copied())
    }
    /// Bookmark line strictly before `from_line`, wrapping to the last
    /// bookmark when none lies before the cursor. Returns `None` when no
    /// bookmarks exist.
    pub fn previous_bookmark_line(&self, from_line: usize) -> Option<usize> {
        if self.bookmarks.is_empty() {
            return None;
        }
        self.bookmarks
            .iter()
            .copied()
            .rev()
            .find(|line| *line < from_line)
            .or_else(|| self.bookmarks.last().copied())
    }
    pub(crate) fn clear_marked_range(&mut self) {
        self.marked_range = None;
    }
    pub(crate) fn preferred_goal(&self) -> Option<CursorGoal> {
        self.selection.movement_goal_for(self.selection.primary_index())
    }
    pub(crate) fn preferred_column(&self) -> Option<usize> {
        self.selection.movement_column_for(self.selection.primary_index())
    }
    pub(crate) fn preferred_goal_for_selection(&self, selection_index: usize) -> Option<CursorGoal> {
        self.selection.movement_goal_for(selection_index)
    }
    pub fn visible_column_for_selection(&self, selection_index: usize) -> Option<usize> {
        self.selection.visible_column_for(selection_index)
    }
    pub(crate) fn set_preferred_column(&mut self, preferred_column: Option<usize>) {
        self.set_preferred_goal(preferred_column.map(CursorGoal::Column));
    }
    pub(crate) fn set_preferred_goal(&mut self, goal: Option<CursorGoal>) {
        self.selection.set_all_movement_goals(goal);
    }
    pub(crate) fn clear_preferred_column(&mut self) {
        self.selection.clear_goals();
    }
    pub fn modified(&self) -> bool {
        self.content_epoch != self.saved_content_epoch
    }
    pub fn display_name(&self) -> String {
        self.origin
            .path()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.name_hint.clone())
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Returns the delta since the last call (or since the tab was created)
    /// and resets the internal record to `Unchanged`. Intended for consumers
    /// that mirror buffer state (e.g. an incremental tree-sitter parser).
    pub fn take_buffer_delta(&mut self) -> BufferDelta {
        std::mem::take(&mut self.buffer_delta)
    }
    fn record_full_replace(&mut self) {
        self.buffer_delta = BufferDelta::FullReplace;
    }
    fn record_edits(&mut self, changes: &[TextChange]) {
        // If a prior delta has not yet been consumed, downgrade to
        // FullReplace — compositing two edit batches in pre-batch coords
        // would require re-mapping the second through the first, and that
        // edge case only fires when the consumer skips a revision tick.
        if !matches!(self.buffer_delta, BufferDelta::Unchanged) {
            self.buffer_delta = BufferDelta::FullReplace;
            return;
        }
        self.buffer_delta = BufferDelta::Edits(
            changes
                .iter()
                .map(|change| BufferEdit {
                    range: change.range.clone(),
                    replacement: change.replacement.clone(),
                })
                .collect(),
        );
    }
    fn touch_content(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.line_cache = None;
    }
    fn touch_text_content(&mut self) {
        self.content_epoch = self.next_content_epoch;
        self.next_content_epoch = self.next_content_epoch.saturating_add(1);
        self.touch_content();
    }
    fn mark_current_content_saved(&mut self) {
        self.saved_content_epoch = self.content_epoch;
    }
    pub fn len_chars(&self) -> usize {
        self.buffer.len_chars()
    }
    pub fn line_count(&self) -> usize {
        self.buffer.len_lines().max(1)
    }
    pub fn buffer_text(&self) -> String {
        self.buffer.to_string()
    }
    pub fn is_blank(&self) -> bool {
        self.buffer.chars().all(char::is_whitespace)
    }
    pub fn cursor_char(&self) -> usize {
        self.selection().cursor()
    }
    pub fn cursor_position(&self) -> Position {
        char_to_position(&self.buffer, self.cursor_char())
    }
    pub fn selected_range(&self) -> Range<usize> {
        self.selection().range()
    }
    pub fn has_selection(&self) -> bool {
        self.selection().has_selection()
    }
    pub fn selected_text(&self) -> Option<String> {
        self.has_selection()
            .then(|| self.buffer.slice(self.selection().range()).to_string())
    }
    pub fn lines(&mut self) -> Arc<[DisplayLine]> {
        if let Some(cache) = &self.line_cache {
            if cache.revision == self.revision {
                return Arc::clone(&cache.lines);
            }
        }
        let lines: Arc<[DisplayLine]> = (0..self.buffer.len_lines())
            .map(|line_ix| DisplayLine::from(crate::selection::line_display_text(&self.buffer, line_ix)))
            .collect::<Vec<_>>()
            .into();
        self.line_cache = Some(CachedLines {
            revision: self.revision,
            lines: Arc::clone(&lines),
        });
        lines
    }
    pub(crate) fn select_all(&mut self) {
        let end = self.len_chars();
        self.selection.set_single(Selection::from_range(0..end, false));
        self.marked_range = None;
        self.history.break_current_group();
    }
    pub(crate) fn set_cursor_position(&mut self, position: Position, select_from: Option<Position>) {
        let head = position_to_char(&self.buffer, position);
        match select_from {
            Some(anchor) => {
                let anchor = position_to_char(&self.buffer, anchor);
                self.selection.set_single(normalized_selection_for_buffer(
                    &self.buffer,
                    Selection::new(anchor, head),
                ));
            }
            None => self.move_to(head),
        }
        self.marked_range = None;
        self.history.break_current_group();
    }
    pub(crate) fn set_selection(&mut self, selection: Selection) {
        self.selection
            .set_single(normalized_selection_for_buffer(&self.buffer, selection));
        self.marked_range = None;
        self.history.break_current_group();
    }
    pub(crate) fn set_selection_set(&mut self, selection_set: SelectionSet) {
        self.selection
            .replace_set(normalized_selection_set_for_buffer(&self.buffer, selection_set));
        self.marked_range = None;
        self.history.break_current_group();
    }
    pub(crate) fn set_normalized_selection_set(&mut self, selection_set: SelectionSet) {
        #[cfg(feature = "internal-invariants")]
        {
            debug_assert_eq!(
                normalized_selection_set_for_buffer(&self.buffer, selection_set.clone()),
                selection_set
            );
        }
        self.selection.replace_set(selection_set);
        self.marked_range = None;
        self.history.break_current_group();
    }
    pub(crate) fn set_selection_state(&mut self, selection_state: SelectionState) {
        self.selection = normalized_selection_state_for_buffer(&self.buffer, selection_state);
        self.marked_range = None;
        self.history.break_current_group();
    }
    // Cycles through abandoned redo paths so a fresh edit no longer permanently
    // strands the previous redo branch. Returns false when there is nothing to
    // swap — current redo stack is left untouched.
    pub(crate) fn swap_redo_branch(&mut self) -> bool {
        self.history.swap_redo_branch()
    }
    pub(crate) fn redo_branch_count(&self) -> usize {
        self.history.redo_branch_count()
    }
    pub(crate) fn move_to(&mut self, offset: usize) {
        let offset = crate::selection::floor_grapheme_boundary(&self.buffer, offset);
        self.selection.set_single(Selection::collapsed(offset));
        self.marked_range = None;
        self.history.break_current_group();
    }
    pub(crate) fn select_to(&mut self, offset: usize) {
        let offset = crate::selection::floor_grapheme_boundary(&self.buffer, offset);
        let mut selection = self.selection();
        selection.select_to(offset);
        self.selection
            .set_single(normalized_selection_for_buffer(&self.buffer, selection));
        self.marked_range = None;
        self.history.break_current_group();
    }
    pub(crate) fn apply_edit_request(&mut self, request: EditRequest) -> EditOutcome {
        let len = self.len_chars();
        let normalized_changes = request.changes.normalized_for_len(len);
        let changes = normalized_changes.as_slice().to_vec();
        let changes_text = changes_modify_text(&self.buffer, &changes);
        let selection_before = self.selection.clone();
        let marked_range_before = self.marked_range.clone();
        if changes_text {
            let snapshot_before = self
                .history
                .needs_snapshot(request.kind, request.boundary)
                .then(|| self.history_snapshot());
            self.history
                .record_edit(request.kind, request.boundary, snapshot_before);
        }
        self.apply_normalized_change(
            changes,
            changes_text,
            normalized_changes.primary_inserted_range(),
            request.selection_after,
            request.marked_range_after,
        );
        EditOutcome {
            text_changed: changes_text,
            selection_changed: self.selection != selection_before,
            marked_range_changed: self.marked_range != marked_range_before,
        }
    }
    fn apply_normalized_change(
        &mut self,
        changes: Vec<TextChange>,
        changes_text: bool,
        primary_inserted_range: Range<usize>,
        selection_after: SelectionAfter,
        marked_range_after: Option<Range<usize>>,
    ) {
        let mut cached_lines = if changes_text {
            self.line_cache
                .take()
                .filter(|cache| cache.revision == self.revision)
                // The rendered canvas can retain the previous outer slice,
                // so clone only Arc pointers here. Unchanged line text remains
                // shared instead of copying every String in the document.
                .map(|cache| cache.lines.iter().cloned().collect::<Vec<_>>())
        } else {
            None
        };
        if changes_text {
            self.record_edits(&changes);
            remap_bookmarks_after_changes(&self.buffer, &mut self.bookmarks, &changes);
            for change in changes.iter().rev() {
                if let Some(lines) = cached_lines.as_mut() {
                    if !apply_change_to_cached_lines(lines, &self.buffer, change) {
                        // Couldn't update incrementally — drop the cache so
                        // the next `lines()` call rebuilds from the buffer
                        // rather than serving stale text under the new revision.
                        cached_lines = None;
                    }
                }
                apply_change_to_buffer(&mut self.buffer, change);
            }
        }
        let selection = selection_after_edit(selection_after, primary_inserted_range.clone(), &self.buffer);
        self.selection.replace_set(selection);
        self.marked_range = marked_range_after_edit(marked_range_after, primary_inserted_range, self.len_chars());
        if changes_text {
            self.last_edit_position = Some(self.selection().head().min(self.len_chars()));
            self.touch_text_content();
            if let Some(lines) = cached_lines {
                self.line_cache = Some(CachedLines {
                    revision: self.revision,
                    lines: lines.into(),
                });
            }
        }
    }
    pub fn last_edit_position(&self) -> Option<usize> {
        self.last_edit_position.map(|pos| pos.min(self.len_chars()))
    }
    pub(crate) fn reset_from_disk(&mut self, text: &str) {
        self.buffer = Rope::from_str(text);
        self.move_to(0);
        self.touch_text_content();
        self.record_full_replace();
        self.mark_current_content_saved();
        self.marked_range = None;
        self.history.clear();
        self.last_edit_position = None;
        self.refresh_language();
    }
    pub(crate) fn reset_from_disk_at_path(&mut self, path: PathBuf, text: &str, file_stamp: FileStamp) {
        self.origin.mark_saved(path, file_stamp);
        self.reset_from_disk(text);
    }
    pub(crate) fn mark_autosaved(&mut self, file_stamp: FileStamp, saved_body: &str) {
        self.apply_saved_body(saved_body);
        self.mark_current_content_saved();
        self.origin.update_file_stamp(file_stamp);
    }
    pub(crate) fn refresh_file_stamp_if_path(&mut self, path: &Path, file_stamp: FileStamp) -> bool {
        if self.path().map(PathBuf::as_path) != Some(path) {
            return false;
        }
        self.origin.update_file_stamp(file_stamp);
        true
    }
    pub(crate) fn observe_committed_body_if_path(
        &mut self,
        path: &Path,
        file_stamp: FileStamp,
        saved_body: &str,
    ) -> bool {
        if self.path().map(PathBuf::as_path) != Some(path) {
            return false;
        }
        self.origin.update_file_stamp(file_stamp);
        if self.buffer.slice(..) == saved_body {
            self.mark_current_content_saved();
        } else {
            // The write committed after the editor moved to another history
            // state. Reserve, but never assign, an epoch for the disk body so
            // no existing undo snapshot can incorrectly appear clean.
            self.saved_content_epoch = self.next_content_epoch;
            self.next_content_epoch = self.next_content_epoch.saturating_add(1);
        }
        true
    }
    pub(crate) fn suppress_file_conflict(&mut self, stamp: FileStamp) {
        self.origin.suppress_file_conflict(stamp);
    }
    pub(crate) fn mark_backing_file_missing(&mut self) {
        self.origin.mark_backing_file_missing();
    }
    fn refresh_language(&mut self) -> bool {
        let language = match self.language_mode {
            LanguageMode::Auto => self.detect_language(),
            LanguageMode::PlainText => None,
            LanguageMode::Language(language) => Some(language),
        };
        let changed = self.language != language;
        self.language = language;
        changed
    }
    fn detect_language(&self) -> Option<Language> {
        language::detect(
            self.path().map(PathBuf::as_path),
            Some(first_line_for_detection(&self.buffer).as_str()),
        )
    }
    pub(crate) fn undo(&mut self) -> bool {
        self.history_step(false)
    }
    pub(crate) fn redo(&mut self) -> bool {
        self.history_step(true)
    }
    fn history_step(&mut self, redo: bool) -> bool {
        let current = self.history_snapshot();
        let Some(snapshot) = (if redo {
            self.history.redo(current)
        } else {
            self.history.undo(current)
        }) else {
            return false;
        };
        self.restore_history_snapshot(snapshot);
        true
    }
    fn history_snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            text: self.buffer.clone(),
            selection: self.selection.clone(),
            content_epoch: self.content_epoch,
            bookmarks: self.bookmarks.clone(),
        }
    }
    fn restore_history_snapshot(&mut self, snapshot: HistorySnapshot) {
        self.buffer = snapshot.text;
        self.selection = snapshot.selection;
        self.content_epoch = snapshot.content_epoch;
        self.next_content_epoch = self.next_content_epoch.max(snapshot.content_epoch.saturating_add(1));
        self.bookmarks = snapshot.bookmarks;
        self.marked_range = None;
        self.touch_content();
        self.record_full_replace();
    }
    pub(crate) fn mark_saved_if_current(
        &mut self,
        path: PathBuf,
        revision: u64,
        file_stamp: FileStamp,
        saved_body: &str,
    ) -> bool {
        if self.revision != revision || self.path() != Some(&path) {
            return false;
        }
        self.apply_saved_body(saved_body);
        self.origin.mark_saved(path, file_stamp);
        self.refresh_language();
        self.mark_current_content_saved();
        true
    }
    pub(crate) fn mark_saved_as_if_current(
        &mut self,
        path: PathBuf,
        revision: u64,
        file_stamp: FileStamp,
        saved_body: &str,
    ) -> bool {
        if self.revision != revision {
            return false;
        }
        self.apply_saved_body(saved_body);
        self.origin.mark_saved_as(path, file_stamp);
        self.refresh_language();
        self.mark_current_content_saved();
        true
    }
    fn apply_saved_body(&mut self, saved_body: &str) {
        if self.buffer.slice(..) == saved_body {
            return;
        }
        self.buffer = Rope::from_str(saved_body);
        self.selection = normalized_selection_state_for_buffer(&self.buffer, self.selection.clone());
        self.marked_range = marked_range_after_edit(self.marked_range.clone(), 0..0, self.len_chars());
        self.touch_text_content();
        self.record_full_replace();
    }
}
/// Returns `false` when the cache cannot be incrementally updated
/// (out-of-range line indices or empty cache) so the caller can drop the
/// stale cache rather than stamping it under the new revision.
fn apply_change_to_cached_lines(lines: &mut Vec<DisplayLine>, buffer: &Rope, change: &TextChange) -> bool {
    if lines.is_empty() {
        return false;
    }

    let len_chars = buffer.len_chars();
    let start = change.range.start.min(len_chars);
    let end = change.range.end.min(len_chars);
    let start_line = buffer.char_to_line(start);
    let end_line = buffer.char_to_line(end);
    if start_line >= lines.len() || end_line >= lines.len() {
        return false;
    }

    let start_col = start.saturating_sub(buffer.line_to_char(start_line));
    let end_col = end.saturating_sub(buffer.line_to_char(end_line));
    // Columns are raw rope offsets (line terminators included), but the cached
    // lines have trailing CR/LF stripped. When an endpoint lands inside a
    // multi-char terminator — the `\n` of a `\r\n`, which ropey groups into the
    // preceding line — the column overshoots the stripped line and the splice
    // would silently grab the whole line as prefix/suffix, corrupting the cache.
    // Drop to a full rebuild instead of stamping a garbled line under the new
    // revision.
    if start_col > lines[start_line].chars().count() || end_col > lines[end_line].chars().count() {
        return false;
    }
    let prefix = line_prefix_chars(lines[start_line].as_ref(), start_col);
    let suffix = line_suffix_chars(lines[end_line].as_ref(), end_col);
    let replacement_lines = replacement_display_lines(&change.replacement);

    let mut new_lines = Vec::with_capacity(replacement_lines.len().max(1));
    if replacement_lines.len() == 1 {
        new_lines.push(DisplayLine::from(format!(
            "{}{}{}",
            prefix, replacement_lines[0], suffix
        )));
    } else {
        new_lines.push(DisplayLine::from(format!("{}{}", prefix, replacement_lines[0])));
        new_lines.extend(
            replacement_lines[1..replacement_lines.len() - 1]
                .iter()
                .map(|line| DisplayLine::from(line.as_str())),
        );
        let last = replacement_lines.last().map(String::as_str).unwrap_or("");
        new_lines.push(DisplayLine::from(format!("{last}{suffix}")));
    }

    lines.splice(start_line..=end_line, new_lines);
    if lines.is_empty() {
        lines.push(DisplayLine::from(""));
    }
    true
}

fn replacement_display_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

fn line_prefix_chars(line: &str, char_count: usize) -> String {
    let byte = byte_index_for_char(line, char_count);
    line[..byte].to_string()
}

fn line_suffix_chars(line: &str, char_count: usize) -> String {
    let byte = byte_index_for_char(line, char_count);
    line[byte..].to_string()
}

fn byte_index_for_char(text: &str, char_ix: usize) -> usize {
    if char_ix == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_ix)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

fn first_line_for_detection(buffer: &Rope) -> String {
    buffer.line(0).to_string().trim_end_matches(['\r', '\n']).to_string()
}
fn changes_modify_text(buffer: &Rope, changes: &[TextChange]) -> bool {
    changes
        .iter()
        .any(|change| buffer.slice(change.range.clone()) != change.replacement.as_str())
}
fn remap_bookmarks_after_changes(buffer: &Rope, bookmarks: &mut Vec<usize>, changes: &[TextChange]) {
    if bookmarks.is_empty() {
        return;
    }
    let mut mapped = bookmarks.iter().copied().map(|line| line as isize).collect::<Vec<_>>();
    let mut line_delta = 0isize;
    for change in changes {
        let start_char = change.range.start.min(buffer.len_chars());
        let end_char = change.range.end.min(buffer.len_chars());
        let start_line = buffer.char_to_line(start_char);
        let removed = buffer.char_to_line(end_char).saturating_sub(start_line);
        let inserted = change.replacement.chars().filter(|ch| *ch == '\n').count();
        let start = start_line as isize + line_delta;
        let end = start + removed as isize;
        let shift = inserted as isize - removed as isize;
        let insertion_before_start_line = removed == 0 && inserted > 0 && start_char == buffer.line_to_char(start_line);
        for line in &mut mapped {
            if *line < start {
                continue;
            }
            if insertion_before_start_line {
                *line += inserted as isize;
                continue;
            }
            if *line > end {
                *line += shift;
            } else {
                *line = start;
            }
        }
        line_delta += shift;
    }
    let line_count = (buffer.len_lines() as isize + line_delta).max(1) as usize;
    let last_line = line_count.saturating_sub(1);
    *bookmarks = mapped
        .into_iter()
        .filter(|line| *line >= 0)
        .map(|line| (line as usize).min(last_line))
        .collect();
    bookmarks.sort_unstable();
    bookmarks.dedup();
}
fn normalized_selection_for_buffer(buffer: &Rope, selection: Selection) -> Selection {
    let range = selection.range();
    if range.start == range.end {
        return Selection::collapsed(crate::selection::floor_grapheme_boundary(buffer, selection.cursor()));
    }
    let start = crate::selection::floor_grapheme_boundary(buffer, range.start);
    let end = crate::selection::ceil_grapheme_boundary(buffer, range.end);
    Selection::from_range(start..end, selection.is_reversed())
}
fn normalized_selection_set_for_buffer(buffer: &Rope, selection_set: SelectionSet) -> SelectionSet {
    let selections = selection_set
        .as_slice()
        .iter()
        .map(|selection| normalized_selection_for_buffer(buffer, *selection))
        .collect::<Vec<_>>();
    SelectionSet::from_selections_coalescing_cursors(selections, selection_set.primary_index())
        .unwrap_or_else(|_| coalesced_normalized_selection_set(buffer, selection_set))
}
fn coalesced_normalized_selection_set(buffer: &Rope, selection_set: SelectionSet) -> SelectionSet {
    let mut merged: Vec<(Range<usize>, bool, bool)> = Vec::new();
    for (index, selection) in selection_set.as_slice().iter().enumerate() {
        let normalized = normalized_selection_for_buffer(buffer, *selection);
        let range = normalized.range();
        let is_primary = index == selection_set.primary_index();
        if let Some((last_range, last_reversed, last_primary)) = merged.last_mut() {
            if range.start <= last_range.end {
                last_range.end = last_range.end.max(range.end);
                if is_primary {
                    *last_reversed = normalized.is_reversed();
                    *last_primary = true;
                }
                continue;
            }
        }
        merged.push((range, normalized.is_reversed(), is_primary));
    }
    let primary = merged.iter().position(|(_, _, is_primary)| *is_primary).unwrap_or(0);
    let selections = merged
        .into_iter()
        .map(|(range, reversed, _)| Selection::from_range(range, reversed))
        .collect::<Vec<_>>();
    SelectionSet::from_selections(selections, primary).unwrap_or_else(|_| SelectionSet::single(Selection::collapsed(0)))
}
fn normalized_selection_state_for_buffer(buffer: &Rope, selection_state: SelectionState) -> SelectionState {
    let normalized = normalized_selection_set_for_buffer(buffer, selection_state.selection_set().clone());
    if normalized == *selection_state.selection_set() {
        selection_state
    } else {
        SelectionState::from_set(normalized)
    }
}
fn selection_after_edit(selection_after: SelectionAfter, inserted_range: Range<usize>, buffer: &Rope) -> SelectionSet {
    let len = buffer.len_chars();
    match selection_after {
        SelectionAfter::CollapseToInsertedEnd => {
            SelectionSet::single(Selection::collapsed(inserted_range.end.min(len)))
        }
        SelectionAfter::Exact(selection_set) => selection_set.clamped_to_len(len),
        SelectionAfter::CursorPosition(position) => {
            SelectionSet::single(Selection::collapsed(position_to_char(buffer, position)))
        }
        SelectionAfter::CursorPositionBeforeLineEnd(position) => {
            let line = position.line.min(buffer.len_lines().saturating_sub(1));
            let column = position
                .column
                .min(crate::selection::display_line_char_len(buffer, line).saturating_sub(1));
            SelectionSet::single(Selection::collapsed(position_to_char(
                buffer,
                Position { line, column },
            )))
        }
        SelectionAfter::PositionRange { start, end, reversed } => SelectionSet::single(Selection::from_range(
            position_to_char(buffer, start)..position_to_char(buffer, end),
            reversed,
        )),
        SelectionAfter::InsertedRange { range, reversed } => SelectionSet::single(Selection::from_range(
            inserted_relative_range(inserted_range, range),
            reversed,
        )),
    }
}
fn marked_range_after_edit(
    marked_range_after: Option<Range<usize>>,
    inserted_range: Range<usize>,
    len: usize,
) -> Option<Range<usize>> {
    let range = inserted_relative_range(inserted_range, marked_range_after?);
    let range = clamped_range(range, len);
    (range.start < range.end).then_some(range)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{EditKind, UndoBoundary},
        transaction::{EditRequest, TextChange, TextChangeSet},
    };

    fn tab_with_text(text: &str) -> EditorTab {
        EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("test.rs"), text, None)
    }

    fn cached_lines(tab: &mut EditorTab) -> Vec<String> {
        tab.lines().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn line_cache_updates_single_line_insert_without_full_rebuild() {
        let mut tab = tab_with_text("alpha\nbeta\n");
        assert_eq!(cached_lines(&mut tab), vec!["alpha", "beta", ""]);

        let request = EditRequest::single(EditKind::Insert, UndoBoundary::Merge, 2..2, "Z".to_string());
        tab.apply_edit_request(request);

        assert_eq!(cached_lines(&mut tab), vec!["alZpha", "beta", ""]);
    }

    #[test]
    fn line_cache_updates_multiline_replace() {
        let mut tab = tab_with_text("alpha\nbeta\ngamma");
        let _ = tab.lines();
        let start = tab.buffer().line_to_char(0) + 2;
        let end = tab.buffer().line_to_char(1) + 2;
        let request = EditRequest::from_changes(
            EditKind::Insert,
            UndoBoundary::Break,
            TextChangeSet::single(TextChange::replace(start..end, "X\nY".to_string())),
        );

        tab.apply_edit_request(request);

        assert_eq!(cached_lines(&mut tab), vec!["alX", "Yta", "gamma"]);
    }

    #[test]
    fn line_cache_matches_full_rebuild_for_crlf_boundary_edit() {
        // Editing a CRLF buffer at the `\n` of a `\r\n` pair (which ropey groups
        // into the preceding line) used to corrupt the incremental line cache:
        // the column was computed in raw rope coordinates but applied to the
        // CR/LF-stripped display line, overshooting it. The incremental result
        // must equal a from-scratch rebuild of the resulting text.
        let mut tab = tab_with_text("alpha\r\nbeta\r\ngamma");
        let _ = tab.lines(); // prime the incremental cache
                             // char 6 is the `\n` of the first CRLF pair.
        let request = EditRequest::single(EditKind::Insert, UndoBoundary::Break, 6..6, "X".to_string());
        tab.apply_edit_request(request);

        let incremental = cached_lines(&mut tab);
        let rebuilt = cached_lines(&mut tab_with_text(&tab.buffer_text()));
        assert_eq!(incremental, rebuilt);
    }

    #[test]
    fn undo_restores_rope_snapshot() {
        let mut tab = tab_with_text("alpha\nbeta");
        let request = EditRequest::single(EditKind::Insert, UndoBoundary::Break, 0..0, "Z".to_string());
        tab.apply_edit_request(request);
        assert_eq!(tab.buffer_text(), "Zalpha\nbeta");

        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "alpha\nbeta");
    }

    #[test]
    fn buffer_delta_reports_edits_then_resets() {
        let mut tab = tab_with_text("alpha\nbeta");
        assert!(matches!(tab.take_buffer_delta(), BufferDelta::Unchanged));

        let request = EditRequest::single(EditKind::Insert, UndoBoundary::Merge, 2..2, "Z".to_string());
        tab.apply_edit_request(request);
        let delta = tab.take_buffer_delta();
        let BufferDelta::Edits(edits) = delta else {
            panic!("expected Edits, got {:?}", delta);
        };
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, 2..2);
        assert_eq!(edits[0].replacement, "Z");
        // Taking a second time returns Unchanged.
        assert!(matches!(tab.take_buffer_delta(), BufferDelta::Unchanged));
    }

    #[test]
    fn buffer_delta_collapses_unconsumed_batches_to_full_replace() {
        let mut tab = tab_with_text("alpha");
        let r1 = EditRequest::single(EditKind::Insert, UndoBoundary::Merge, 0..0, "A".to_string());
        let r2 = EditRequest::single(EditKind::Insert, UndoBoundary::Merge, 0..0, "B".to_string());
        tab.apply_edit_request(r1);
        tab.apply_edit_request(r2);
        // Two edit batches without an intervening take_buffer_delta — must
        // collapse to FullReplace so the consumer reparses from scratch.
        assert!(matches!(tab.take_buffer_delta(), BufferDelta::FullReplace));
    }

    #[test]
    fn buffer_delta_reports_full_replace_after_undo() {
        let mut tab = tab_with_text("alpha\nbeta");
        let request = EditRequest::single(EditKind::Insert, UndoBoundary::Break, 0..0, "Z".to_string());
        tab.apply_edit_request(request);
        let _ = tab.take_buffer_delta();
        assert!(tab.undo());
        assert!(matches!(tab.take_buffer_delta(), BufferDelta::FullReplace));
    }
}
