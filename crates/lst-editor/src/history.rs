use crate::{
    document::{EditKind, UndoBoundary},
    selection::SelectionState,
};

const MAX_UNDO: usize = 100;
const MAX_REDO_BRANCHES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistorySnapshot {
    pub(crate) text: String,
    pub(crate) selection: SelectionState,
    pub(crate) content_epoch: u64,
    pub(crate) bookmarks: Vec<usize>,
}

#[derive(Clone)]
pub(crate) struct EditHistory {
    undo_stack: Vec<HistorySnapshot>,
    redo_stack: Vec<HistorySnapshot>,
    // Abandoned redo paths, most-recent last. A fresh edit moves the current
    // redo path here instead of dropping it, so `swap_redo_branch` can pull
    // the latest sibling branch back into reach.
    redo_branches: Vec<Vec<HistorySnapshot>>,
    last_edit_kind: Option<EditKind>,
}

impl EditHistory {
    pub(crate) fn new() -> Self {
        Self { undo_stack: Vec::new(), redo_stack: Vec::new(), redo_branches: Vec::new(), last_edit_kind: None }
    }

    pub(crate) fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.redo_branches.clear();
        self.last_edit_kind = None;
    }

    pub(crate) fn break_current_group(&mut self) {
        self.last_edit_kind = None;
    }

    pub(crate) fn record_edit(&mut self, kind: EditKind, boundary: UndoBoundary, snapshot_before: HistorySnapshot) {
        if self.should_start_undo_group(kind, boundary) {
            self.preserve_redo_branch();
            self.undo_stack.push(snapshot_before);
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
        } else if self.undo_stack.is_empty() {
            self.undo_stack.push(snapshot_before);
        }
        self.last_edit_kind = Some(kind);
    }

    pub(crate) fn undo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        let snapshot = self.undo_stack.pop()?;
        self.redo_stack.push(current);
        self.last_edit_kind = None;
        Some(snapshot)
    }

    pub(crate) fn redo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        let snapshot = self.redo_stack.pop()?;
        self.undo_stack.push(current);
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
}
