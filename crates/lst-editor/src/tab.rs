use crate::{
    document::{char_to_position, position_to_char, EditKind, UndoBoundary},
    language::{self, Language},
    position::Position,
    selection::Selection,
};
use ropey::Rope;
use std::{
    fs::Metadata,
    ops::Range,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_UNDO: usize = 100;
// Each branch can still hold up to MAX_UNDO full-buffer snapshots, so this
// only caps how many alternate timelines we keep — not total memory.
const MAX_REDO_BRANCHES: usize = 8;

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
struct Snapshot {
    text: String,
    selection: Selection,
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
    pub(crate) name_hint: String,
    pub(crate) origin: TabOrigin,
    pub(crate) language: Option<Language>,
    language_override: Option<Option<Language>>,
    pub(crate) buffer: Rope,
    pub(crate) modified: bool,
    pub(crate) selection: Selection,
    pub(crate) preferred_column: Option<usize>,
    revision: u64,
    line_cache: Option<CachedLines>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    // Abandoned redo paths, most-recent last. A fresh edit moves the current
    // redo path here instead of dropping it, so `swap_redo_branch` can pull
    // the latest sibling branch back into reach.
    redo_branches: Vec<Vec<Snapshot>>,
    last_edit_kind: Option<EditKind>,
    last_edit_position: Option<usize>,
    pub marked_range: Option<Range<usize>>,
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
            selection: Selection::collapsed(0),
            preferred_column: None,
            revision: 0,
            line_cache: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            redo_branches: Vec::new(),
            last_edit_kind: None,
            last_edit_position: None,
            marked_range: None,
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

    pub fn scratchpad_path_for_cleanup(&self) -> Option<PathBuf> {
        self.scratchpad_path().cloned()
    }

    pub fn conflict_suppressed_for(&self, stamp: FileStamp) -> bool {
        self.origin.conflict_suppressed_for(stamp)
    }

    pub fn buffer(&self) -> &Rope {
        &self.buffer
    }

    pub fn buffer_clone(&self) -> Rope {
        self.buffer.clone()
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn selection_reversed(&self) -> bool {
        self.selection.is_reversed()
    }

    pub fn marked_range(&self) -> Option<&Range<usize>> {
        self.marked_range.as_ref()
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
        self.selection.cursor()
    }

    pub fn cursor_position(&self) -> Position {
        char_to_position(&self.buffer, self.cursor_char())
    }

    pub fn selected_range(&self) -> Range<usize> {
        self.selection.range()
    }

    pub fn has_selection(&self) -> bool {
        self.selection.has_selection()
    }

    pub fn selected_text(&self) -> Option<String> {
        if self.has_selection() {
            Some(self.buffer.slice(self.selection.range()).to_string())
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

    pub fn select_all(&mut self) {
        let end = self.len_chars();
        self.selection = Selection::from_range(0..end, false);
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
                self.selection = Selection::new(anchor, head);
            }
            None => self.move_to(head),
        }
        self.marked_range = None;
    }

    pub(crate) fn set_selection(&mut self, selection: Selection) {
        let len = self.len_chars();
        let anchor = selection.anchor().min(len);
        let head = selection.head().min(len);
        self.selection = Selection::new(anchor, head);
        self.preferred_column = None;
        self.marked_range = None;
    }

    pub(crate) fn push_undo_snapshot(&mut self, kind: EditKind, boundary: UndoBoundary) {
        let kind_changed = self.last_edit_kind != Some(kind);
        let is_streaming = matches!(kind, EditKind::Insert | EditKind::Delete);
        let should_snapshot =
            kind_changed || !is_streaming || matches!(boundary, UndoBoundary::Break);

        if should_snapshot {
            self.undo_stack.push(self.current_snapshot());
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
            if !self.redo_stack.is_empty() {
                let abandoned = std::mem::take(&mut self.redo_stack);
                self.redo_branches.push(abandoned);
                if self.redo_branches.len() > MAX_REDO_BRANCHES {
                    self.redo_branches.remove(0);
                }
            }
        }
        self.last_edit_kind = Some(kind);
    }

    // Cycles through abandoned redo paths so a fresh edit no longer permanently
    // strands the previous redo branch. Returns false when there is nothing to
    // swap — current redo stack is left untouched.
    pub fn swap_redo_branch(&mut self) -> bool {
        let Some(branch) = self.redo_branches.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.redo_stack, branch);
        if !current.is_empty() {
            self.redo_branches.insert(0, current);
        }
        true
    }

    pub fn redo_branch_count(&self) -> usize {
        self.redo_branches.len()
    }

    pub fn move_to(&mut self, offset: usize) {
        let offset = offset.min(self.len_chars());
        self.selection.move_to(offset);
        self.marked_range = None;
    }

    pub fn select_to(&mut self, offset: usize) {
        let offset = offset.min(self.len_chars());
        self.selection.select_to(offset);
        self.marked_range = None;
    }

    pub fn replace_char_range(&mut self, mut range: Range<usize>, new_text: &str) -> usize {
        range.start = range.start.min(self.len_chars());
        range.end = range.end.min(self.len_chars());
        if range.start > range.end {
            range = range.end..range.start;
        }

        if range.start != range.end {
            self.buffer.remove(range.clone());
        }
        if !new_text.is_empty() {
            self.buffer.insert(range.start, new_text);
        }

        let new_cursor = range.start + new_text.chars().count();
        self.selection.move_to(new_cursor);
        self.modified = true;
        self.preferred_column = None;
        self.marked_range = None;
        self.last_edit_position = Some(new_cursor);
        self.touch_content();
        new_cursor
    }

    // Returns None until the buffer has been edited; clamps to the current
    // length so callers never need to re-validate after undo/redo trims text.
    pub fn last_edit_position(&self) -> Option<usize> {
        self.last_edit_position.map(|pos| pos.min(self.len_chars()))
    }

    pub fn edit(
        &mut self,
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        new_text: &str,
    ) -> usize {
        self.push_undo_snapshot(kind, boundary);
        self.replace_char_range(range, new_text)
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer = Rope::from_str(text);
        self.move_to(0);
        self.modified = false;
        self.preferred_column = None;
        self.marked_range = None;
        self.touch_content();
        self.last_edit_kind = None;
        self.last_edit_position = None;
    }

    pub(crate) fn reset_from_disk(&mut self, text: &str) {
        self.buffer = Rope::from_str(text);
        self.move_to(0);
        self.modified = false;
        self.preferred_column = None;
        self.marked_range = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.redo_branches.clear();
        self.refresh_language();
        self.touch_content();
        self.last_edit_kind = None;
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

    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    fn current_snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.buffer_text(),
            selection: self.selection,
        }
    }

    fn restore_snapshot(&mut self, snapshot: Snapshot) {
        self.buffer = Rope::from_str(&snapshot.text);
        self.selection = snapshot.selection;
        self.preferred_column = None;
        self.marked_range = None;
        self.touch_content();
        self.last_edit_kind = None;
    }
}

fn first_line_for_detection(buffer: &Rope) -> String {
    buffer
        .line(0)
        .to_string()
        .trim_end_matches(['\r', '\n'])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_restore_text_and_selection() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "hello");
        tab.move_to(5);
        tab.edit(EditKind::Insert, UndoBoundary::Merge, 5..5, " world");

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
        tab.edit(EditKind::Insert, UndoBoundary::Break, 0..0, "a");
        tab.edit(EditKind::Insert, UndoBoundary::Merge, 1..1, "b");
        tab.undo();
        assert_eq!(tab.buffer_text(), "");

        tab.edit(EditKind::Insert, UndoBoundary::Break, 0..0, "a");
        tab.edit(EditKind::Insert, UndoBoundary::Break, 1..1, " ");
        tab.undo();
        assert_eq!(tab.buffer_text(), "a");
    }

    #[test]
    fn swap_redo_branch_recovers_abandoned_redo_path() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "");
        tab.edit(EditKind::Insert, UndoBoundary::Break, 0..0, "a");
        tab.edit(EditKind::Insert, UndoBoundary::Break, 1..1, "b");
        assert_eq!(tab.buffer_text(), "ab");
        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "a");

        // A fresh edit normally drops the "ab" redo path; preservation keeps it
        // so the user can recover it via swap_redo_branch.
        tab.edit(EditKind::Insert, UndoBoundary::Break, 1..1, "c");
        assert_eq!(tab.buffer_text(), "ac");
        assert_eq!(tab.redo_branch_count(), 1);

        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "a");
        assert!(tab.swap_redo_branch());
        assert!(tab.redo());
        assert_eq!(tab.buffer_text(), "ab");
    }

    #[test]
    fn swap_redo_branch_cycles_through_multiple_branches() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "a");

        // Type a label, undo, then the next edit abandons that redo path —
        // the loop leaves 3 abandoned branches plus the latest as the active
        // redo path, so 4 distinct timelines should be reachable in total.
        for label in ['X', 'Y', 'Z', 'W'] {
            let end = tab.len_chars();
            tab.edit(
                EditKind::Insert,
                UndoBoundary::Break,
                end..end,
                &label.to_string(),
            );
            assert!(tab.undo());
        }
        assert_eq!(tab.redo_branch_count(), 3);

        let mut seen = Vec::new();
        let collect = |tab: &mut EditorTab, seen: &mut Vec<String>| {
            assert!(tab.redo());
            seen.push(tab.buffer_text());
            assert!(tab.undo());
        };

        collect(&mut tab, &mut seen);
        for _ in 0..3 {
            assert!(tab.swap_redo_branch());
            collect(&mut tab, &mut seen);
        }
        seen.sort();
        assert_eq!(seen, vec!["aW", "aX", "aY", "aZ"]);
    }

    #[test]
    fn swap_redo_branch_returns_false_with_no_branches() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "x");
        tab.edit(EditKind::Insert, UndoBoundary::Break, 1..1, "y");
        assert!(!tab.swap_redo_branch());
    }

    #[test]
    fn last_edit_position_records_caret_after_each_edit() {
        let mut tab = EditorTab::from_text(TabId::from_raw(1), "untitled".into(), None, "abcdef");
        assert_eq!(tab.last_edit_position(), None);

        tab.edit(EditKind::Insert, UndoBoundary::Break, 3..3, "X");
        assert_eq!(tab.last_edit_position(), Some(4));

        tab.edit(EditKind::Insert, UndoBoundary::Break, 0..0, "YY");
        assert_eq!(tab.last_edit_position(), Some(2));
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
