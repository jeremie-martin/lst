use crate::{
    document::{EditKind, UndoBoundary},
    find_scope::CapturedFindScope,
    selection::SelectionState,
    transaction::TextChangeOffsetMap,
};
use ropey::Rope;
use std::collections::{HashMap, VecDeque};

const MAX_UNDO: usize = 10_000;
// The byte budget is deliberately soft: never regress the previous guarantee
// of retaining one hundred groups merely because a document is large.
const MIN_UNDO: usize = 100;
const MAX_ACTIVE_UNDO_ESTIMATED_BYTES: usize = 64 * 1024 * 1024;
const MAX_REDO_BRANCHES: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct HistorySnapshot {
    pub(crate) text: Rope,
    pub(crate) selection: SelectionState,
    pub(crate) content_epoch: u64,
    pub(crate) bookmarks: Vec<usize>,
    pub(crate) find_scope: Option<CapturedFindScope>,
}

#[derive(Clone)]
struct HistoryEntry {
    snapshot: HistorySnapshot,
    other_epoch: u64,
    // Forward edits: snapshot -> other on undo, other -> snapshot on redo.
    // None marks a jump to a branch whose connecting history was evicted.
    changes: Option<Vec<TextChangeOffsetMap>>,
    change_bytes: usize,
}

impl HistoryEntry {
    fn new(snapshot: HistorySnapshot, other_epoch: u64, changes: Option<Vec<TextChangeOffsetMap>>) -> Self {
        let change_bytes = changes
            .as_ref()
            .map_or(0, |c| c.iter().map(TextChangeOffsetMap::estimated_bytes).sum());
        Self {
            snapshot,
            other_epoch,
            changes,
            change_bytes,
        }
    }

    fn append(&mut self, change: TextChangeOffsetMap) {
        let changes = self
            .changes
            .as_mut()
            .expect("live edit groups contain position changes");
        if changes.last_mut().is_some_and(|last| last.extend_streaming(&change)) {
            return;
        }
        self.change_bytes = self.change_bytes.saturating_add(change.estimated_bytes());
        changes.push(change);
    }

    fn estimated_bytes(&self) -> usize {
        let spare_maps = self.changes.as_ref().map_or(0, |c| {
            (c.capacity() - c.len()) * std::mem::size_of::<TextChangeOffsetMap>()
        });
        self.snapshot
            .estimated_bytes()
            .saturating_add(std::mem::size_of::<Self>() - std::mem::size_of::<HistorySnapshot>())
            .saturating_add(self.change_bytes)
            .saturating_add(spare_maps)
    }
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
            .saturating_add(self.find_scope.as_ref().map_or(0, CapturedFindScope::estimated_bytes))
    }
}

#[derive(Clone)]
pub(crate) struct EditHistory {
    undo_stack: Vec<HistoryEntry>,
    undo_estimated_bytes: usize,
    redo_stack: Vec<HistoryEntry>,
    // Abandoned redo paths, most-recent last. A fresh edit moves the current
    // redo path here instead of dropping it, so `swap_redo_branch` can pull
    // the latest sibling branch back into reach.
    redo_branches: Vec<Vec<HistoryEntry>>,
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

    /// Once a capture ends, its saved ranges can never be useful again.
    /// Clearing them also means every remaining scope belongs to the one
    /// current capture; no generation IDs or shared identity tokens are needed.
    pub(crate) fn clear_find_scopes(&mut self) {
        for entry in &mut self.undo_stack {
            if let Some(scope) = entry.snapshot.find_scope.take() {
                self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_sub(scope.estimated_bytes());
            }
        }
        for entry in self
            .redo_stack
            .iter_mut()
            .chain(self.redo_branches.iter_mut().flatten())
        {
            entry.snapshot.find_scope = None;
        }
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
        changes: TextChangeOffsetMap,
        next_epoch: u64,
    ) {
        if self.needs_snapshot(kind, boundary) {
            let snapshot = snapshot_before.expect("starting an undo group requires a snapshot");
            self.preserve_redo_branch();
            self.push_undo(HistoryEntry::new(snapshot, next_epoch, Some(Vec::new())));
        }
        let entry = self.undo_stack.last_mut().expect("an edit has an undo group");
        self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_sub(entry.estimated_bytes());
        entry.other_epoch = next_epoch;
        entry.append(changes);
        self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_add(entry.estimated_bytes());
        self.trim_undo();
        self.last_edit_kind = Some(kind);
    }

    pub(crate) fn undo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        let entry = self.undo_stack.pop()?;
        self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_sub(entry.estimated_bytes());
        let mut snapshot = entry.snapshot;
        let other_epoch = snapshot.content_epoch;
        restore_scope(&current, &mut snapshot, entry.changes.as_deref(), false);
        self.redo_stack
            .push(HistoryEntry::new(current, other_epoch, entry.changes));
        self.last_edit_kind = None;
        Some(snapshot)
    }

    pub(crate) fn redo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        let entry = self.redo_stack.last()?;
        let changes = if entry.other_epoch == current.content_epoch {
            entry.changes.clone()
        } else {
            self.position_path(current.content_epoch, entry.snapshot.content_epoch)
        };
        let mut snapshot = self.redo_stack.pop()?.snapshot;
        restore_scope(&current, &mut snapshot, changes.as_deref(), true);
        self.push_undo(HistoryEntry::new(current, snapshot.content_epoch, changes));
        self.last_edit_kind = None;
        Some(snapshot)
    }

    /// Branches may be selected away from their fork. Traverse retained edit
    /// edges so positions follow the actual path rather than a text diff.
    fn position_path(&self, from: u64, to: u64) -> Option<Vec<TextChangeOffsetMap>> {
        let edges: Vec<_> = self
            .undo_stack
            .iter()
            .map(|e| (e.snapshot.content_epoch, e.other_epoch, e))
            .chain(
                self.redo_stack
                    .iter()
                    .chain(self.redo_branches.iter().flatten())
                    .map(|e| (e.other_epoch, e.snapshot.content_epoch, e)),
            )
            .filter(|(_, _, e)| e.changes.is_some())
            .collect();
        let mut neighbors: HashMap<u64, Vec<(u64, usize, bool)>> = HashMap::new();
        for (i, &(before, after, _)) in edges.iter().enumerate() {
            neighbors.entry(before).or_default().push((after, i, true));
            neighbors.entry(after).or_default().push((before, i, false));
        }
        let mut previous = HashMap::new();
        let mut queue = VecDeque::from([from]);
        previous.insert(from, (from, 0, true));
        while let Some(epoch) = queue.pop_front() {
            if epoch == to {
                break;
            }
            for &(next, edge, forward) in neighbors.get(&epoch).into_iter().flatten() {
                if let std::collections::hash_map::Entry::Vacant(v) = previous.entry(next) {
                    v.insert((epoch, edge, forward));
                    queue.push_back(next);
                }
            }
        }
        let mut path = Vec::new();
        let mut epoch = to;
        while epoch != from {
            let &(parent, edge, forward) = previous.get(&epoch)?;
            path.push((edge, forward));
            epoch = parent;
        }
        let mut changes = Vec::new();
        for (edge, forward) in path.into_iter().rev() {
            let step = edges[edge].2.changes.as_ref().expect("only mapped edges were visited");
            if forward {
                changes.extend_from_slice(step);
            } else {
                changes.extend(step.iter().rev().map(|c| c.inverted()));
            }
        }
        Some(changes)
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

    fn push_undo(&mut self, snapshot: HistoryEntry) {
        self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_add(snapshot.estimated_bytes());
        self.undo_stack.push(snapshot);
        self.trim_undo();
    }

    fn trim_undo(&mut self) {
        while self.undo_stack.len() > MIN_UNDO
            && (self.undo_stack.len() > MAX_UNDO || self.undo_estimated_bytes > MAX_ACTIVE_UNDO_ESTIMATED_BYTES)
        {
            let removed = self.undo_stack.remove(0);
            self.undo_estimated_bytes = self.undo_estimated_bytes.saturating_sub(removed.estimated_bytes());
        }
    }
}

fn restore_scope(
    current: &HistorySnapshot,
    target: &mut HistorySnapshot,
    changes: Option<&[TextChangeOffsetMap]>,
    forward: bool,
) {
    let Some(scope) = current.find_scope.as_ref() else {
        target.find_scope = None;
        return;
    };
    if target.find_scope.is_some() {
        return;
    }
    let mut mapped = scope.clone();
    if let Some(changes) = changes {
        mapped.map_history(changes, forward);
    } else {
        mapped.clear();
    }
    target.find_scope = Some(mapped);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        transaction::{TextChange, TextChangeSet},
        Selection, SelectionSet,
    };

    fn snapshot(epoch: u64, scope: Option<CapturedFindScope>) -> HistorySnapshot {
        HistorySnapshot {
            text: Rope::from_str("same same"),
            selection: SelectionState::single(Selection::collapsed(0)),
            content_epoch: epoch,
            bookmarks: Vec::new(),
            find_scope: scope,
        }
    }

    #[test]
    fn an_evicted_branch_connection_cannot_expand_a_capture_to_document_scope() {
        let scope = CapturedFindScope::capture(&SelectionSet::single(Selection::new(5, 9))).unwrap();
        let mut history = EditHistory::new();
        // The old branch survives, but its connection to the current state
        // has fallen outside retained undo history.
        history.redo_stack.push(HistoryEntry::new(
            snapshot(1, None),
            0,
            Some(vec![
                TextChangeSet::single(TextChange::insert(0, "old ")).into_parts().1,
            ]),
        ));
        let old = history.redo(snapshot(100, Some(scope))).unwrap();
        let captured = old.find_scope.as_ref().expect("still selection-scoped");
        assert!(!captured.contains(&(0..0)));
        assert!(!captured.contains(&(5..9)));
        let restored = history.undo(old).unwrap();
        let captured = restored.find_scope.unwrap();
        assert!(captured.contains(&(5..9)));
        assert!(!captured.contains(&(0..4)));
    }
}
