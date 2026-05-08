use crate::{
    document::{char_to_position, position_to_char},
    history::{EditHistory, HistorySnapshot},
    language::{self, Language},
    position::Position,
    selection::{CursorGoal, Selection, SelectionSet, SelectionState},
    transaction::{
        apply_change_to_buffer, clamped_range, inserted_relative_range, EditOutcome, EditRequest,
        SelectionAfter, TextChange,
    },
};
use ropey::Rope;
use std::{
    fs::Metadata,
    ops::Range,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

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

fn system_time_to_unix_nanos(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos().min(i128::MAX as u128) as i128,
        Err(err) => -(err.duration().as_nanos().min(i128::MAX as u128) as i128),
    }
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
    lines: Arc<[String]>,
}

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
        suppressed_conflict_stamp: Option<FileStamp>,
    },
}

impl TabOrigin {
    fn saved(path: PathBuf, file_stamp: Option<FileStamp>, kind: SaveKind) -> Self {
        Self::Saved {
            path,
            file_stamp,
            kind,
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

    pub fn conflict_suppressed_for(&self, stamp: FileStamp) -> bool {
        match self {
            Self::Untitled => false,
            Self::Saved {
                suppressed_conflict_stamp,
                ..
            } => *suppressed_conflict_stamp == Some(stamp),
        }
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
            suppressed_conflict_stamp,
            ..
        } = self
        {
            *stamp = Some(file_stamp);
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
    language_override: Option<Option<Language>>,
    buffer: Rope,
    modified: bool,
    selection: SelectionState,
    revision: u64,
    line_cache: Option<CachedLines>,
    history: EditHistory,
    last_edit_position: Option<usize>,
    marked_range: Option<Range<usize>>,
    /// Bookmarked logical lines, sorted ascending. Stored as raw line
    /// numbers — no anchor tracking, so bookmarks drift on edits.
    bookmarks: Vec<usize>,
}

impl EditorTab {
    pub fn empty(id: TabId, name_hint: String) -> Self {
        Self::from_text(id, name_hint, None, "")
    }

    pub fn from_path(id: TabId, path: PathBuf, text: &str) -> Self {
        Self::from_path_with_stamp(id, path, text, None)
    }

    pub fn from_path_with_stamp(
        id: TabId,
        path: PathBuf,
        text: &str,
        file_stamp: Option<FileStamp>,
    ) -> Self {
        let name_hint = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled")
            .to_string();
        Self::from_text_with_stamp(id, name_hint, Some(path), text, file_stamp)
    }

    pub fn scratchpad_with_stamp(id: TabId, path: PathBuf, file_stamp: FileStamp) -> Self {
        let name_hint = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("scratchpad")
            .to_string();
        Self::from_origin(
            id,
            name_hint,
            TabOrigin::saved(path, Some(file_stamp), SaveKind::Scratchpad),
            "",
        )
    }

    pub fn from_text(id: TabId, name_hint: String, path: Option<PathBuf>, text: &str) -> Self {
        Self::from_text_with_stamp(id, name_hint, path, text, None)
    }

    pub fn from_text_with_stamp(
        id: TabId,
        name_hint: String,
        path: Option<PathBuf>,
        text: &str,
        file_stamp: Option<FileStamp>,
    ) -> Self {
        let origin = match path {
            Some(path) => TabOrigin::saved(path, file_stamp, SaveKind::Regular),
            None => TabOrigin::Untitled,
        };
        Self::from_origin(id, name_hint, origin, text)
    }

    fn from_origin(id: TabId, name_hint: String, origin: TabOrigin, text: &str) -> Self {
        let language =
            language::detect(origin.path().map(PathBuf::as_path), text.split('\n').next());
        Self {
            id,
            name_hint,
            origin,
            language,
            language_override: None,
            buffer: Rope::from_str(text),
            modified: false,
            selection: SelectionState::single(Selection::collapsed(0)),
            revision: 0,
            line_cache: None,
            history: EditHistory::new(),
            last_edit_position: None,
            marked_range: None,
            bookmarks: Vec::new(),
        }
    }

    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.origin.path()
    }

    pub fn language(&self) -> Option<Language> {
        self.language
    }

    pub fn language_config(&self) -> &'static crate::language::LanguageConfig {
        language::config_for(self.language)
    }

    pub(crate) fn set_language(&mut self, language: Option<Language>) {
        self.language_override = Some(language);
        self.language = language;
        self.touch_content();
    }

    pub fn file_stamp(&self) -> Option<FileStamp> {
        self.origin.file_stamp()
    }

    pub fn is_scratchpad(&self) -> bool {
        self.origin.is_scratchpad()
    }

    pub fn scratchpad_path(&self) -> Option<&PathBuf> {
        self.is_scratchpad().then(|| self.path()).flatten()
    }

    pub fn conflict_suppressed_for(&self, stamp: FileStamp) -> bool {
        self.origin.conflict_suppressed_for(stamp)
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

    pub fn bookmarks(&self) -> &[usize] {
        &self.bookmarks
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
        self.selection
            .movement_goal_for(self.selection.primary_index())
    }

    pub(crate) fn preferred_column(&self) -> Option<usize> {
        self.selection
            .movement_column_for(self.selection.primary_index())
    }

    pub(crate) fn preferred_goal_for_selection(
        &self,
        selection_index: usize,
    ) -> Option<CursorGoal> {
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
        self.modified
    }

    pub fn display_name(&self) -> String {
        self.origin
            .path()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.name_hint.clone())
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn touch_content(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.line_cache = None;
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
        if self.has_selection() {
            Some(self.buffer.slice(self.selection().range()).to_string())
        } else {
            None
        }
    }

    pub fn lines(&mut self) -> Arc<[String]> {
        if let Some(cache) = &self.line_cache {
            if cache.revision == self.revision {
                return Arc::clone(&cache.lines);
            }
        }

        let lines: Arc<[String]> = self
            .buffer
            .to_string()
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
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
        self.selection
            .set_single(Selection::from_range(0..end, false));
        self.marked_range = None;
    }

    pub(crate) fn set_cursor_position(
        &mut self,
        position: Position,
        select_from: Option<Position>,
    ) {
        let head = position_to_char(&self.buffer, position);
        match select_from {
            Some(anchor) => {
                let anchor = position_to_char(&self.buffer, anchor);
                self.selection.set_single(Selection::new(anchor, head));
            }
            None => self.move_to(head),
        }
        self.marked_range = None;
    }

    pub(crate) fn set_selection(&mut self, selection: Selection) {
        let len = self.len_chars();
        let anchor = selection.anchor().min(len);
        let head = selection.head().min(len);
        self.selection.set_single(Selection::new(anchor, head));
        self.marked_range = None;
    }

    pub(crate) fn set_selection_set(&mut self, selection_set: SelectionSet) {
        self.selection
            .replace_set(selection_set.clamped_to_len(self.len_chars()));
        self.marked_range = None;
    }

    pub(crate) fn set_selection_state(&mut self, selection_state: SelectionState) {
        self.selection = selection_state.clamped_to_len(self.len_chars());
        self.marked_range = None;
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
        let offset = offset.min(self.len_chars());
        self.selection.set_single(Selection::collapsed(offset));
        self.marked_range = None;
    }

    pub(crate) fn select_to(&mut self, offset: usize) {
        let offset = offset.min(self.len_chars());
        let mut selection = self.selection();
        selection.select_to(offset);
        self.selection.set_single(selection);
        self.marked_range = None;
    }

    pub(crate) fn apply_edit_request(&mut self, request: EditRequest) -> EditOutcome {
        let len = self.len_chars();
        let normalized_changes = request.changes.normalized_for_len(len);
        let changes = normalized_changes.as_slice().to_vec();
        let changes_text = changes_modify_text(&self.buffer, &changes);
        let selection_before = self.selection.clone();
        let marked_range_before = self.marked_range.clone();
        if changes_text {
            self.history
                .record_edit(request.kind, request.boundary, self.history_snapshot());
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
        if changes_text {
            for change in changes.iter().rev() {
                apply_change_to_buffer(&mut self.buffer, change);
            }
        }
        let selection = selection_after_edit(
            selection_after,
            primary_inserted_range.clone(),
            &self.buffer,
        );
        self.selection.replace_set(selection);
        self.marked_range =
            marked_range_after_edit(marked_range_after, primary_inserted_range, self.len_chars());
        if changes_text {
            self.modified = true;
            self.touch_content();
        }

        let last_edit_position = self.selection().head().min(self.len_chars());
        if changes_text {
            self.last_edit_position = Some(last_edit_position);
        }
    }

    // Returns None until the buffer has been edited; clamps to the current
    // length so callers never need to re-validate after undo/redo trims text.
    pub fn last_edit_position(&self) -> Option<usize> {
        self.last_edit_position.map(|pos| pos.min(self.len_chars()))
    }

    pub(crate) fn reset_from_disk(&mut self, text: &str) {
        self.buffer = Rope::from_str(text);
        self.move_to(0);
        self.modified = false;
        self.marked_range = None;
        self.history.clear();
        self.refresh_language();
        self.touch_content();
        self.last_edit_position = None;
    }

    pub(crate) fn mark_saved(&mut self, path: PathBuf, file_stamp: FileStamp) {
        self.origin.mark_saved(path, file_stamp);
        self.refresh_language();
        self.modified = false;
    }

    pub(crate) fn mark_saved_as(&mut self, path: PathBuf, file_stamp: FileStamp) {
        self.origin.mark_saved_as(path, file_stamp);
        self.refresh_language();
        self.modified = false;
    }

    pub(crate) fn reset_from_disk_at_path(
        &mut self,
        path: PathBuf,
        text: &str,
        file_stamp: FileStamp,
    ) {
        self.origin.mark_saved(path, file_stamp);
        self.reset_from_disk(text);
    }

    pub(crate) fn mark_modified(&mut self) {
        self.modified = true;
    }

    pub(crate) fn mark_autosaved(&mut self, file_stamp: FileStamp) {
        self.modified = false;
        self.origin.update_file_stamp(file_stamp);
    }

    pub(crate) fn suppress_file_conflict(&mut self, stamp: FileStamp) {
        self.origin.suppress_file_conflict(stamp);
    }

    fn refresh_language(&mut self) -> bool {
        if self.language_override.is_some() {
            return false;
        }

        let language = self.detect_language();
        let changed = self.language != language;
        self.language = language;
        changed
    }

    fn detect_language(&self) -> Option<Language> {
        let first_line = first_line_for_detection(&self.buffer);
        language::detect(self.path().map(PathBuf::as_path), Some(first_line.as_str()))
    }

    pub(crate) fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.history.undo(self.history_snapshot()) {
            self.restore_history_snapshot(snapshot);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    pub(crate) fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.history.redo(self.history_snapshot()) {
            self.restore_history_snapshot(snapshot);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    fn history_snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            text: self.buffer_text(),
            selection: self.selection.clone(),
        }
    }

    fn restore_history_snapshot(&mut self, snapshot: HistorySnapshot) {
        self.buffer = Rope::from_str(&snapshot.text);
        self.selection = snapshot.selection;
        self.marked_range = None;
        self.touch_content();
    }
}

fn first_line_for_detection(buffer: &Rope) -> String {
    buffer
        .line(0)
        .to_string()
        .trim_end_matches(['\r', '\n'])
        .to_string()
}

fn changes_modify_text(buffer: &Rope, changes: &[TextChange]) -> bool {
    changes
        .iter()
        .any(|change| buffer.slice(change.range.clone()) != change.replacement.as_str())
}

fn selection_after_edit(
    selection_after: SelectionAfter,
    inserted_range: Range<usize>,
    buffer: &Rope,
) -> SelectionSet {
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
        SelectionAfter::PositionRange {
            start,
            end,
            reversed,
        } => SelectionSet::single(Selection::from_range(
            position_to_char(buffer, start)..position_to_char(buffer, end),
            reversed,
        )),
        SelectionAfter::InsertedRange { range, reversed } => SelectionSet::single(
            Selection::from_range(inserted_relative_range(inserted_range, range), reversed),
        ),
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
    use crate::document::{EditKind, UndoBoundary};

    fn edit(
        tab: &mut EditorTab,
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        text: &str,
    ) -> usize {
        tab.apply_edit_request(EditRequest::single(kind, boundary, range, text.to_string()));
        tab.selection().head()
    }

    #[test]
    fn undo_and_redo_restore_text_and_selection() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "hello");
        tab.move_to(5);
        edit(
            &mut tab,
            EditKind::Insert,
            UndoBoundary::Merge,
            5..5,
            " world",
        );

        assert_eq!(tab.buffer_text(), "hello world");
        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "hello");
        assert_eq!(tab.cursor_char(), 5);
        assert!(tab.redo());
        assert_eq!(tab.buffer_text(), "hello world");
    }

    #[test]
    fn undo_boundaries_split_streaming_edits() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "");
        edit(&mut tab, EditKind::Insert, UndoBoundary::Break, 0..0, "a");
        edit(&mut tab, EditKind::Insert, UndoBoundary::Merge, 1..1, "b");
        tab.undo();
        assert_eq!(tab.buffer_text(), "");

        edit(&mut tab, EditKind::Insert, UndoBoundary::Break, 0..0, "a");
        edit(&mut tab, EditKind::Insert, UndoBoundary::Break, 1..1, " ");
        tab.undo();
        assert_eq!(tab.buffer_text(), "a");
    }

    #[test]
    fn swap_redo_branch_recovers_abandoned_redo_path() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "");
        edit(&mut tab, EditKind::Insert, UndoBoundary::Break, 0..0, "a");
        edit(&mut tab, EditKind::Insert, UndoBoundary::Break, 1..1, "b");
        assert_eq!(tab.buffer_text(), "ab");
        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "a");

        // A fresh edit normally drops the "ab" redo path; preservation keeps it
        // so the user can recover it via swap_redo_branch.
        edit(&mut tab, EditKind::Insert, UndoBoundary::Break, 1..1, "c");
        assert_eq!(tab.buffer_text(), "ac");
        assert_eq!(tab.redo_branch_count(), 1);

        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "a");
        assert!(tab.swap_redo_branch());
        assert!(tab.redo());
        assert_eq!(tab.buffer_text(), "ab");
    }

    #[test]
    fn no_op_edit_request_updates_selection_without_dirtying_history() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "abc");
        let revision_before = tab.revision();
        let request = EditRequest::single(EditKind::Other, UndoBoundary::Break, 0..1, "a".into())
            .with_selection_after(SelectionAfter::Exact(SelectionSet::single(
                Selection::collapsed(2),
            )));

        let outcome = tab.apply_edit_request(request);

        assert_eq!(tab.buffer_text(), "abc");
        assert_eq!(tab.selection(), Selection::collapsed(2));
        assert_eq!(tab.revision(), revision_before);
        assert_eq!(
            outcome,
            EditOutcome {
                text_changed: false,
                selection_changed: true,
                marked_range_changed: false,
            }
        );
        assert!(!tab.modified());
        assert_eq!(tab.last_edit_position(), None);
        assert!(!tab.undo());
    }

    #[cfg(feature = "internal-invariants")]
    #[test]
    fn edit_request_clamps_reversed_range_and_updates_revision() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "abcdef");
        tab.set_selection(Selection::from_range(2..4, false));

        let outcome = tab.apply_edit_request(EditRequest::single(
            EditKind::Other,
            UndoBoundary::Break,
            Range { start: 8, end: 1 },
            "X".to_string(),
        ));

        assert!(outcome.text_changed);
        assert_eq!(tab.buffer_text(), "aX");
        assert_eq!(tab.selection(), Selection::collapsed(2));
        assert_eq!(tab.revision(), 1);
        assert_eq!(tab.last_edit_position(), Some(2));
    }

    #[test]
    fn edit_request_applies_ime_selection_and_marked_range_relative_to_insert() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "");
        let request = EditRequest::single(
            EditKind::Other,
            UndoBoundary::Break,
            0..0,
            "a🙂b".to_string(),
        )
        .with_selection_after(SelectionAfter::InsertedRange {
            range: 1..2,
            reversed: false,
        })
        .with_marked_range_after(0..3);

        tab.apply_edit_request(request);

        assert_eq!(tab.buffer_text(), "a🙂b");
        assert_eq!(tab.selection(), Selection::from_range(1..2, false));
        assert_eq!(tab.marked_range(), Some(&(0..3)));
    }

    #[cfg(feature = "internal-invariants")]
    #[test]
    fn edit_request_applies_multi_change_transaction() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "abcd");
        let changes = crate::transaction::TextChangeSet::new(
            vec![
                TextChange::replace(0..1, "A"),
                TextChange::replace(2..3, "C"),
            ],
            0,
        );
        let request = EditRequest::from_changes(EditKind::Other, UndoBoundary::Break, changes)
            .with_selection_after(SelectionAfter::Exact(SelectionSet::single(
                Selection::collapsed(3),
            )));

        let outcome = tab.apply_edit_request(request);

        assert!(outcome.text_changed);
        assert_eq!(tab.buffer_text(), "AbCd");
        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "abcd");
        assert_eq!(tab.selection(), Selection::collapsed(0));
        assert!(tab.redo());
        assert_eq!(tab.buffer_text(), "AbCd");
        assert_eq!(tab.selection(), Selection::collapsed(3));
    }

    #[test]
    fn lines_strip_cr_from_crlf_content() {
        let mut tab = EditorTab::from_text(
            TabId::from_raw(1),
            "untitled".into(),
            None,
            "alpha\r\nbeta\r\n",
        );

        assert_eq!(tab.lines().as_ref(), ["alpha", "beta", ""]);
    }
}
