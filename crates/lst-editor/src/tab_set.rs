use crate::tab::{EditorTab, TabId};
use std::ops::{Deref, DerefMut};

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

    pub(crate) fn push(&mut self, tab: EditorTab) {
        self.next_tab_id = self.next_tab_id.max(tab.id().get().saturating_add(1));
        self.tabs.push(tab);
    }

    pub(crate) fn replace_only(&mut self, tab: EditorTab) {
        self.tabs.clear();
        self.next_tab_id = self.next_tab_id.max(tab.id().get().saturating_add(1));
        self.tabs.push(tab);
        self.active = 0;
    }

    pub(crate) fn remove(&mut self, index: usize) -> EditorTab {
        assert!(self.tabs.len() > 1, "TabSet cannot remove its last tab");
        let removed = self.tabs.remove(index);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        removed
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

impl DerefMut for TabSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tabs
    }
}

fn next_id_after(tabs: &[EditorTab]) -> u64 {
    tabs.iter()
        .map(|tab| tab.id().get())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, name: &str) -> EditorTab {
        EditorTab::from_text(TabId::from_raw(id), name.to_string(), None, "")
    }

    fn build(active: usize, names: &[&str]) -> TabSet {
        let mut tabs = names.iter().enumerate().map(|(i, n)| tab(i as u64 + 1, n));
        let first = tabs.next().expect("non-empty");
        let mut set = TabSet::new(first, tabs.collect());
        assert!(set.activate(active));
        set
    }

    fn names(set: &TabSet) -> Vec<&str> {
        set.iter().map(|tab| tab.name_hint.as_str()).collect()
    }

    #[test]
    fn reorder_shifts_active_left_when_tab_drags_past_it() {
        // active = "b" at index 1; moving "a" from 0 to 2 shifts active left.
        let mut set = build(1, &["a", "b", "c"]);
        assert!(set.reorder(0, 2));
        assert_eq!(names(&set), vec!["b", "c", "a"]);
        assert_eq!(set.active_index(), 0);
    }

    #[test]
    fn reorder_shifts_active_right_when_tab_drags_in_front_of_it() {
        // active = "b" at index 1; moving "c" from 2 to 0 shifts active right.
        let mut set = build(1, &["a", "b", "c"]);
        assert!(set.reorder(2, 0));
        assert_eq!(names(&set), vec!["c", "a", "b"]);
        assert_eq!(set.active_index(), 2);
    }

    #[test]
    fn reorder_leaves_active_alone_when_move_is_outside_active_range() {
        // active = "b" at index 1; reordering c→d range (indices 2 and 3) leaves it.
        let mut set = build(1, &["a", "b", "c", "d"]);
        assert!(set.reorder(2, 3));
        assert_eq!(names(&set), vec!["a", "b", "d", "c"]);
        assert_eq!(set.active_index(), 1);
    }

    #[test]
    fn reorder_returns_false_for_no_op_or_out_of_range() {
        let mut set = build(0, &["a", "b"]);
        assert!(!set.reorder(0, 0));
        assert!(!set.reorder(0, 9));
        assert!(!set.reorder(9, 0));
    }
}
