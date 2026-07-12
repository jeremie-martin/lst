use crate::{
    document::{EditKind, UndoBoundary},
    selection::SelectionState,
};
use ropey::Rope;

const MAX_UNDO: usize = 10_000;
// The byte budget is deliberately soft: never regress the previous guarantee
// of retaining one hundred groups merely because a document is large.
const MIN_UNDO: usize = 100;
const MAX_ACTIVE_UNDO_ESTIMATED_BYTES: usize = 64 * 1024 * 1024;
const MAX_REDO_BRANCHES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistorySnapshot {
    pub(crate) text: Rope,
    pub(crate) selection: SelectionState,
    pub(crate) content_epoch: u64,
    pub(crate) bookmarks: Vec<usize>,
}

impl HistorySnapshot {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            // Rope snapshots share their tree, so this deliberately uses the
            // O(1) logical byte count as a conservative upper bound. Calling
            // Rope::capacity here would walk every chunk on each edit.
            .saturating_add(self.text.len_bytes())
            .saturating_add(
                self.selection
                    .selection_set()
                    .as_slice()
                    .len()
                    .saturating_mul(std::mem::size_of::<crate::Selection>()),
            )
            .saturating_add(self.bookmarks.capacity().saturating_mul(std::mem::size_of::<usize>()))
    }
}

#[derive(Clone)]
pub(crate) struct EditHistory {
    undo_stack: Vec<HistorySnapshot>,
    undo_estimated_bytes: usize,
    redo_stack: Vec<HistorySnapshot>,
    // Abandoned redo paths, most-recent last. A fresh edit moves the current
    // redo path here instead of dropping it, so `swap_redo_branch` can pull
    // the latest sibling branch back into reach.
    redo_branches: Vec<Vec<HistorySnapshot>>,
    last_edit_kind: Option<EditKind>,
}

impl EditHistory {
    pub(crate) fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            undo_estimated_bytes: 0,
            redo_stack: Vec::new(),
            redo_branches: Vec::new(),
            last_edit_kind: None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.undo_stack.clear();
        self.undo_estimated_bytes = 0;
        self.redo_stack.clear();
        self.redo_branches.clear();
        self.last_edit_kind = None;
    }

    pub(crate) fn break_current_group(&mut self) {
        self.last_edit_kind = None;
    }

    pub(crate) fn needs_snapshot(&self, kind: EditKind, boundary: UndoBoundary) -> bool {
        self.should_start_undo_group(kind, boundary) || self.undo_stack.is_empty()
    }

    pub(crate) fn record_edit(
        &mut self,
        kind: EditKind,
        boundary: UndoBoundary,
        snapshot_before: Option<HistorySnapshot>,
    ) {
        if self.should_start_undo_group(kind, boundary) {
            let snapshot_before = snapshot_before.expect("starting an undo group requires a snapshot");
            self.preserve_redo_branch();
            self.push_undo(snapshot_before);
        } else if self.undo_stack.is_empty() {
            let snapshot_before = snapshot_before.expect("first merged edit requires a snapshot");
            self.push_undo(snapshot_before);
        }
        self.last_edit_kind = Some(kind);
    }

    pub(crate) fn undo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        let snapshot = self.undo_stack.pop()?;
        self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_sub(snapshot.estimated_bytes());
        self.redo_stack.push(current);
        self.last_edit_kind = None;
        Some(snapshot)
    }

    pub(crate) fn redo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        let snapshot = self.redo_stack.pop()?;
        self.push_undo(current);
        self.last_edit_kind = None;
        Some(snapshot)
    }

    pub(crate) fn swap_redo_branch(&mut self) -> bool {
        let Some(branch) = self.redo_branches.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.redo_stack, branch);
        if !current.is_empty() {
            self.redo_branches.insert(0, current);
        }
        true
    }

    pub(crate) fn redo_branch_count(&self) -> usize {
        self.redo_branches.len()
    }

    fn should_start_undo_group(&self, kind: EditKind, boundary: UndoBoundary) -> bool {
        let kind_changed = self.last_edit_kind != Some(kind);
        let is_streaming = matches!(kind, EditKind::Insert | EditKind::Delete);
        kind_changed || !is_streaming || matches!(boundary, UndoBoundary::Break)
    }

    fn preserve_redo_branch(&mut self) {
        if !self.redo_stack.is_empty() {
            let abandoned = std::mem::take(&mut self.redo_stack);
            self.redo_branches.push(abandoned);
            if self.redo_branches.len() > MAX_REDO_BRANCHES {
                self.redo_branches.remove(0);
            }
        }
    }

    fn push_undo(&mut self, snapshot: HistorySnapshot) {
        self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_add(snapshot.estimated_bytes());
        self.undo_stack.push(snapshot);
        while self.undo_stack.len() > MIN_UNDO
            && (self.undo_stack.len() > MAX_UNDO || self.undo_estimated_bytes > MAX_ACTIVE_UNDO_ESTIMATED_BYTES)
        {
            let removed = self.undo_stack.remove(0);
            self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_sub(removed.estimated_bytes());
        }
    }
}
