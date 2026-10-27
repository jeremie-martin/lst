use crate::{EditorEffect, EditorModel, EditorTab, FileStamp, RevealIntent, TabId};
use std::path::PathBuf;

impl EditorModel {
    pub(crate) fn request_open_files(&mut self) {
        self.queue_effect(EditorEffect::OpenFiles);
    }

    pub fn open_files_with_stamps(&mut self, files: Vec<(PathBuf, String, Option<FileStamp>)>) {
        let mut opened = 0;
        let mut last_opened = None;
        for (path, text, file_stamp) in files {
            last_opened = Some(
                self.tabs
                    .push_new(|id| EditorTab::from_path_with_stamp(id, path, &text, file_stamp)),
            );
            opened += 1;
        }
        if let Some(index) = last_opened {
            self.activate_tab(index);
            self.status = format!("Opened {opened} tab(s).");
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }
    pub fn open_file_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to open {}: {message}", path.display());
    }
    pub(crate) fn request_save(&mut self) {
        self.request_save_tab(self.active_tab_id());
    }

    pub fn request_save_tab(&mut self, tab_id: TabId) {
        let Some(tab) = self.tab_by_id(tab_id) else {
            return;
        };
        if let Some(path) = tab.path().cloned() {
            self.queue_effect(EditorEffect::SaveFile {
                tab_id,
                path,
                body: tab.buffer_text(),
                revision: tab.revision(),
                expectation: tab.save_expectation(),
            });
        } else {
            self.request_save_as_tab(tab_id);
        }
    }

    pub(crate) fn request_save_as(&mut self) {
        self.request_save_as_tab(self.active_tab_id());
    }

    pub fn request_save_as_tab(&mut self, tab_id: TabId) {
        let Some(tab) = self.tab_by_id(tab_id) else {
            return;
        };
        self.queue_effect(EditorEffect::SaveFileAs {
            tab_id,
            suggested_name: tab.display_name().to_owned(),
            body: tab.buffer_text(),
            revision: tab.revision(),
            previous_scratchpad_path: tab.scratchpad_path().cloned(),
        });
    }

    pub fn save_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        file_stamp: FileStamp,
        saved_body: crate::PreparedSave,
    ) -> bool {
        let Some(tab) = self.tab_mut_by_id(tab_id) else {
            return false;
        };
        if !tab.mark_saved_if_current(path.clone(), revision, file_stamp, &saved_body) {
            tab.observe_committed_body_if_path(&path, file_stamp, saved_body.text());
            return false;
        }
        if self.active_tab_id() == tab_id {
            self.ensure_find_matches_current();
        }
        self.status = format!("Saved {}.", path.display());
        true
    }

    pub fn save_as_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        file_stamp: FileStamp,
        saved_body: crate::PreparedSave,
    ) -> bool {
        let Some(tab) = self.tab_mut_by_id(tab_id) else {
            return false;
        };
        if !tab.mark_saved_as_if_current(path.clone(), revision, file_stamp, &saved_body) {
            return false;
        }
        if self.active_tab_id() == tab_id {
            self.ensure_find_matches_current();
        }
        self.status = format!("Saved {}.", path.display());
        true
    }
    pub fn save_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to save {}: {message}", path.display());
    }

    pub fn autosave_tick(&mut self, include_ordinary_files: bool, eligible_tabs: &[TabId]) {
        let jobs = self
            .tabs
            .iter()
            .filter(|tab| eligible_tabs.contains(&tab.id()))
            .filter(|tab| tab.modified())
            .filter(|tab| include_ordinary_files || tab.is_scratchpad())
            .filter_map(|tab| {
                let path = tab.path().cloned()?;
                let open_tabs_for_path = self
                    .tabs
                    .iter()
                    .filter(|candidate| candidate.path() == Some(&path))
                    .take(2)
                    .count();
                if open_tabs_for_path != 1 {
                    return None;
                }
                Some((
                    tab.id(),
                    path,
                    tab.buffer_text(),
                    tab.revision(),
                    tab.save_expectation(),
                ))
            })
            .collect::<Vec<_>>();
        for (tab_id, path, body, revision, expectation) in jobs {
            self.queue_effect(EditorEffect::AutosaveFile {
                tab_id,
                path,
                body,
                revision,
                expectation,
            });
        }
    }

    pub fn autosave_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        file_stamp: FileStamp,
        saved_body: crate::PreparedSave,
    ) -> bool {
        let active_id = self.active_tab_id();
        let Some(tab) = self.tab_mut_by_id(tab_id) else {
            return false;
        };
        if tab.path() != Some(&path) {
            return false;
        }
        if tab.revision() != revision {
            tab.observe_committed_body_if_path(&path, file_stamp, saved_body.text());
            return false;
        }
        tab.mark_autosaved(file_stamp, &saved_body);
        if tab_id == active_id {
            self.ensure_find_matches_current();
            self.status = format!("Autosaved {}.", path.display());
        }
        true
    }
    pub fn autosave_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Autosave failed for {}: {message}", path.display());
    }

    pub fn refresh_file_stamp_for_tab(&mut self, tab_id: TabId, path: PathBuf, file_stamp: FileStamp) -> bool {
        let Some(tab) = self.tab_mut_by_id(tab_id) else {
            return false;
        };
        tab.refresh_file_stamp_if_path(&path, file_stamp)
    }

    pub fn reload_tab_from_disk(&mut self, tab_id: TabId, path: PathBuf, text: String, file_stamp: FileStamp) -> bool {
        let active = self.active_tab_id() == tab_id;
        {
            let Some(tab) = self.tab_mut_by_id(tab_id) else {
                return false;
            };
            if tab.path() != Some(&path) {
                return false;
            }
            let previous_position = tab.cursor_position();
            tab.reset_from_disk_at_path(path.clone(), &text, file_stamp);
            tab.set_cursor_position(previous_position, None);
        }
        self.sync_find_with_active_document();
        self.status = format!("Reloaded {}.", path.display());
        if active {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
        true
    }
    pub fn suppress_file_conflict(&mut self, tab_id: TabId, path: PathBuf, stamp: FileStamp) {
        if let Some(tab) = self.tab_mut_by_id(tab_id).filter(|tab| tab.path() == Some(&path)) {
            tab.suppress_file_conflict(stamp);
            self.status = format!("Kept local changes for {}.", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SaveTextOptions;

    #[test]
    fn old_path_results_cannot_reload_or_suppress_conflicts_after_save_as() {
        let tab_id = TabId::from_raw(1);
        let tab = EditorTab::from_path_with_stamp(tab_id, "old.txt".into(), "original", None);
        let mut model = EditorModel::from_tabs(tab, Vec::new(), "Ready".into());
        let revision = model.active_tab().revision();
        let saved_stamp = FileStamp::from_raw(8, Some(1));
        assert!(model.save_as_finished_for_tab(
            tab_id,
            "new.txt".into(),
            revision,
            saved_stamp,
            SaveTextOptions::default().prepare("original".into()),
        ));
        let stale_stamp = FileStamp::from_raw(3, Some(2));
        assert!(!model.reload_tab_from_disk(tab_id, "old.txt".into(), "old".into(), stale_stamp));
        model.suppress_file_conflict(tab_id, "old.txt".into(), stale_stamp);
        assert_eq!(model.active_tab().path(), Some(&PathBuf::from("new.txt")));
        assert_eq!(model.active_tab().buffer_text(), "original");
        assert_eq!(model.active_tab().file_stamp(), Some(saved_stamp));
        assert!(!model.active_tab().conflict_suppressed_for(stale_stamp));
    }
}
