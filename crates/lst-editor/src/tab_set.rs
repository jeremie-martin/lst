use crate::tab::{EditorTab, TabId};
use std::{collections::HashSet, ops::Deref};

pub(crate) struct TabSet {
    tabs: Vec<EditorTab>,
    active: usize,
    ids: TabIds,
}

impl TabSet {
    pub(crate) fn new(first: EditorTab, rest: Vec<EditorTab>) -> Self {
        let mut tabs = Vec::with_capacity(rest.len() + 1);
        tabs.push(first);
        tabs.extend(rest);
        let mut ids = TabIds::new(&tabs);
        repair_duplicate_tab_ids(&mut tabs, &mut ids);
        Self { tabs, active: 0, ids }
    }

    pub(crate) fn active(&self) -> &EditorTab {
        &self.tabs[self.active]
    }

    pub(crate) fn active_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active]
    }

    pub(crate) fn active_index(&self) -> usize {
        self.active
    }

    pub(crate) fn activate(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.active != index {
            self.active_mut().clear_find_scope();
        }
        self.active = index;
        true
    }

    pub(crate) fn push_new(&mut self, make_tab: impl FnOnce(TabId) -> EditorTab) -> usize {
        let index = self.tabs.len();
        let id = self.ids.allocate();
        let tab = make_tab(id);
        assert_eq!(tab.id(), id, "new tab must use its allocated ID");
        self.tabs.push(tab);
        index
    }

    pub(crate) fn replace_only_with_new(&mut self, make_tab: impl FnOnce(TabId) -> EditorTab) {
        let id = self.ids.allocate();
        let tab = make_tab(id);
        assert_eq!(tab.id(), id, "new tab must use its allocated ID");
        self.tabs.clear();
        self.tabs.push(tab);
        self.active = 0;
    }

    pub(crate) fn remove(&mut self, index: usize) -> bool {
        assert!(self.tabs.len() > 1, "TabSet cannot remove its last tab");
        let removed_active = index == self.active;
        self.tabs.remove(index);
        if removed_active {
            self.active = self.active.min(self.tabs.len() - 1);
        } else if index < self.active {
            self.active -= 1;
        }
        removed_active
    }

    // The active tab moves with its content so reorder feels like dragging
    // the same tab, never like swapping which tab is focused.
    pub(crate) fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if self.active == from {
            self.active = to;
        } else if from < self.active && to >= self.active {
            self.active -= 1;
        } else if from > self.active && to <= self.active {
            self.active += 1;
        }
        true
    }

    pub(crate) fn tab_by_id(&self, tab_id: TabId) -> Option<&EditorTab> {
        self.tabs.iter().find(|tab| tab.id() == tab_id)
    }

    pub(crate) fn tab_mut_by_id(&mut self, tab_id: TabId) -> Option<&mut EditorTab> {
        self.tabs.iter_mut().find(|tab| tab.id() == tab_id)
    }

    pub(crate) fn index_by_id(&self, tab_id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id() == tab_id)
    }
}

impl Deref for TabSet {
    type Target = [EditorTab];

    fn deref(&self) -> &Self::Target {
        &self.tabs
    }
}

// Initial IDs stay reserved after their tabs close: asynchronous results may
// still refer to them. Generated IDs are never reused during this model's life.
struct TabIds {
    initial: HashSet<TabId>,
    start: u64,
    next: Option<u64>,
}

impl TabIds {
    fn new(tabs: &[EditorTab]) -> Self {
        let initial: HashSet<_> = tabs.iter().map(EditorTab::id).collect();
        let start = initial.iter().map(|id| id.get()).max().unwrap_or(0).wrapping_add(1);
        Self {
            initial,
            start,
            next: Some(start),
        }
    }

    fn allocate(&mut self) -> TabId {
        loop {
            let raw = self.next.expect("tab ID space exhausted");
            let following = raw.wrapping_add(1);
            self.next = (following != self.start).then_some(following);
            let id = TabId::from_raw(raw);
            if !self.initial.contains(&id) {
                return id;
            }
        }
    }
}

fn repair_duplicate_tab_ids(tabs: &mut [EditorTab], ids: &mut TabIds) {
    let mut seen = HashSet::with_capacity(tabs.len());
    for tab in tabs {
        if !seen.insert(tab.id()) {
            tab.set_id(ids.allocate());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64) -> EditorTab {
        EditorTab::empty(TabId::from_raw(id), format!("tab-{id}"))
    }

    #[test]
    fn duplicate_and_maximum_initial_ids_are_repaired_without_reusing_initial_ids() {
        let mut tabs = TabSet::new(tab(u64::MAX), vec![tab(u64::MAX), tab(0), tab(1)]);
        let ids: Vec<_> = tabs.iter().map(|tab| tab.id().get()).collect();
        assert_eq!(ids, vec![u64::MAX, 2, 0, 1]);
        tabs.remove(2);
        tabs.push_new(|id| EditorTab::empty(id, "new".into()));
        assert_eq!(tabs.last().unwrap().id().get(), 3);
        assert_ne!(tabs.last().unwrap().id().get(), 0);
    }

    #[test]
    fn allocation_wraps_once_without_reusing_closed_or_generated_ids() {
        let mut tabs = TabSet::new(tab(u64::MAX - 1), vec![tab(0)]);
        tabs.push_new(|id| EditorTab::empty(id, "first".into()));
        assert_eq!(tabs.last().unwrap().id().get(), u64::MAX);
        tabs.remove(1);
        tabs.remove(1);
        tabs.push_new(|id| EditorTab::empty(id, "second".into()));
        assert_eq!(tabs.last().unwrap().id().get(), 1);
        tabs.replace_only_with_new(|id| EditorTab::empty(id, "replacement".into()));
        assert_eq!(tabs.active().id().get(), 2);
    }
}
