use crate::tab::{EditorTab, TabId};
use std::{collections::HashSet, ops::Deref};

pub(crate) struct TabSet {
    tabs: Vec<EditorTab>,
    active: usize,
    next_tab_id: u64,
}

impl TabSet {
    pub(crate) fn new(first: EditorTab, rest: Vec<EditorTab>) -> Self {
        let mut tabs = Vec::with_capacity(rest.len() + 1);
        tabs.push(first);
        tabs.extend(rest);
        repair_duplicate_tab_ids(&mut tabs);
        let next_tab_id = next_id_after(&tabs);
        Self {
            tabs,
            active: 0,
            next_tab_id,
        }
    }

    pub(crate) fn alloc_tab_id(&mut self) -> TabId {
        let id = TabId::from_raw(self.next_tab_id);
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        id
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
        self.active = index;
        true
    }

    pub(crate) fn push(&mut self, tab: EditorTab) -> usize {
        let index = self.tabs.len();
        self.next_tab_id = self.next_tab_id.max(tab.id().get().saturating_add(1));
        self.tabs.push(tab);
        index
    }

    pub(crate) fn replace_only(&mut self, tab: EditorTab) {
        self.tabs.clear();
        self.next_tab_id = self.next_tab_id.max(tab.id().get().saturating_add(1));
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

fn repair_duplicate_tab_ids(tabs: &mut [EditorTab]) {
    let mut seen = HashSet::with_capacity(tabs.len());
    let mut next_id = next_id_after(tabs);
    for tab in tabs {
        if seen.insert(tab.id()) {
            continue;
        }
        while seen.contains(&TabId::from_raw(next_id)) {
            next_id = next_id.saturating_add(1);
        }
        let repaired = TabId::from_raw(next_id);
        tab.set_id(repaired);
        seen.insert(repaired);
        next_id = next_id.saturating_add(1);
    }
}

fn next_id_after(tabs: &[EditorTab]) -> u64 {
    tabs.iter()
        .map(|tab| tab.id().get())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}
