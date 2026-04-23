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

    pub(crate) fn as_slice(&self) -> &[EditorTab] {
        &self.tabs
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

impl<'a> IntoIterator for &'a TabSet {
    type IntoIter = std::slice::Iter<'a, EditorTab>;
    type Item = &'a EditorTab;

    fn into_iter(self) -> Self::IntoIter {
        self.tabs.iter()
    }
}

impl<'a> IntoIterator for &'a mut TabSet {
    type IntoIter = std::slice::IterMut<'a, EditorTab>;
    type Item = &'a mut EditorTab;

    fn into_iter(self) -> Self::IntoIter {
        self.tabs.iter_mut()
    }
}

fn next_id_after(tabs: &[EditorTab]) -> u64 {
    tabs.iter()
        .map(|tab| tab.id().get())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}
